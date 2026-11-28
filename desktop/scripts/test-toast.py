"""Isolated browser test. Run with Python Playwright and an installed Chrome browser."""
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from threading import Thread
import unittest

from playwright.sync_api import expect, sync_playwright


class StaticUI(SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


class ToastTest(unittest.TestCase):
    def test_feedback(self):
        ui = Path(__file__).resolve().parents[1] / "ui"
        with ThreadingHTTPServer(("127.0.0.1", 0), partial(StaticUI, directory=ui)) as server:
            Thread(target=server.serve_forever, daemon=True).start()
            self.addCleanup(server.shutdown)
            with sync_playwright() as pw:
                browser = pw.chromium.launch(channel="chrome")
                page = browser.new_page(reduced_motion="reduce")
                errors = []
                page.on("pageerror", lambda err: errors.append(str(err)))
                page.add_init_script("""
                  window.listeners = {};
                  window.snapshotReads = 0;
                  const target = {provider:'github',host:'github.com',path:'fixture/demo',branch:'main',bound:true,username:'fixture'};
                  window.__TAURI__ = {event:{listen:async (key,fn)=>{listeners[key]=fn;return ()=>{};}},
                    core:{invoke:async cmd=>{
                      if(cmd==='snapshot') {
                        window.snapshotReads++;
                        if(window.failSnapshot) throw Error('fixture-only');
                        return {entries:[],secrets:[],monitoring:false};
                      }
                      if(cmd==='account_status') return {...target,saved:target};
                      if(cmd==='account_login') return target;
                      if(cmd==='account_sync') return new Promise(resolve=>{window.syncDone=resolve;});
                      if(cmd==='pending_requests') return [];
                    }}};
                """)
                page.goto(f"http://127.0.0.1:{server.server_port}/")
                page.evaluate("""async () => {
                  window.core = await import('./core.js');
                  window.toast = await import('./toast.js');
                  window.lang = await import('./lang.js');
                }""")
                self.assertEqual(errors, [])
                page.wait_for_function("() => window.snapshotReads > 0 && Boolean(listeners['app-locked'])")
                items = page.locator(".ky-toast")
                live = page.locator("#toast-live")
                origin = page.locator('[data-view="account"]').first
                origin.focus()
                page.evaluate("core.setStatus('status.entryCopied',{title:'PRIVATE_FIXTURE'})")
                expect(items).to_have_count(1)
                expect(origin).to_be_focused()
                expect(live).to_be_empty()
                page.evaluate("core.setStatus('status.entryCopied');core.snapshotReady()")
                expect(items).to_have_count(1)
                expect(page.locator('#status')).to_have_text(items.locator('.toast-caption').inner_text())
                page.evaluate("core.setStatus('status.textSaved');toast.notifyToast('PNG',{id:'png'});")
                expect(items).to_have_count(3)
                expect(live).to_have_text("PNG")
                page.evaluate("toast.notifyToast('HTML',{id:'html'})")
                expect(items).to_have_count(3)
                self.assertNotIn("PRIVATE_FIXTURE", page.locator("#toasts").inner_text())
                page.evaluate("lang.applyLanguage('en',{persist:false,notifyBackend:false})")
                expect(items.filter(has_text="Text saved")).to_have_count(1)
                self.assertEqual(items.first.evaluate("e=>getComputedStyle(e).animationName"), "none")

                page.evaluate("toast.clearToasts()")
                page.clock.install()
                page.evaluate("toast.notifyToast('Saved',{id:'timed'})")
                page.clock.fast_forward(5100)
                expect(items).to_have_count(0)
                page.evaluate("toast.notifyToast('Exporting',{id:'export',kind:'pending'})")
                expect(items).to_have_attribute("data-kind", "pending")
                page.evaluate("toast.notifyToast(null,{id:'export',kind:'cancel'})")
                expect(items).to_have_count(0)
                page.evaluate("core.setFormError('#secret-error','err.generic')")
                expect(items).to_have_count(0)
                page.evaluate("core.setFormError('#secret-error',null); core.setError('err.generic')")
                expect(items.get_by_role('button',name='Retry')).to_have_count(0)
                expect(live).to_be_empty()
                close = items.get_by_role('button', name='Dismiss notification')
                close.focus()
                close.press('Escape')
                expect(items).to_have_count(0)
                expect(origin).to_be_focused()
                page.evaluate("core.setError('err.generic')")
                items.get_by_role('button', name='Dismiss notification').click()
                expect(items).to_have_count(0)

                page.evaluate("window.failSnapshot=true;listeners['history-changed']()")
                retry = items.get_by_role('button', name='Retry')
                expect(retry).to_be_visible()
                before = page.evaluate("window.snapshotReads")
                page.evaluate("window.failSnapshot=false")
                retry.click()
                expect(page.locator('#error')).to_be_hidden()
                self.assertEqual(page.evaluate("window.snapshotReads"), before + 1)

                origin.click()
                sync = page.locator('[data-account-field=sync]')
                expect(sync).to_be_enabled()
                sync.click()
                expect(items).to_have_attribute('data-kind','pending')
                expect(sync).to_be_disabled()
                page.evaluate("syncDone('uploaded')")
                expect(items).to_have_attribute('data-kind','success')
                expect(items).to_contain_text('Uploaded', ignore_case=True)
                expect(page.locator('#account [role=status]')).to_contain_text('uploaded')
                expect(live).to_be_empty()
                sync.click()
                page.evaluate("listeners['app-locked']();syncDone('same')")
                expect(page.locator('#locked')).to_be_visible()
                expect(items).to_have_count(0)
                page.evaluate("toast.notifyToast('late',{id:'late'})")
                expect(items).to_have_count(0)
                expect(live).to_be_empty()
                self.assertEqual(errors, [])
                browser.close()


if __name__ == "__main__":
    unittest.main()
