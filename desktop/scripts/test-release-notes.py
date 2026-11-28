from pathlib import Path
from functools import partial
from http.server import SimpleHTTPRequestHandler,ThreadingHTTPServer
from threading import Thread
from playwright.sync_api import sync_playwright,expect
class Quiet(SimpleHTTPRequestHandler):
 def log_message(self,*args):pass
srv=ThreadingHTTPServer(('127.0.0.1',0),partial(Quiet,directory=Path(__file__).resolve().parents[1]/'ui'))
Thread(target=srv.serve_forever,daemon=True).start()
with sync_playwright() as pw:
 b=pw.chromium.launch(channel='chrome',headless=True)
 for width,lang in [(1024,'zh-CN'),(640,'en')]:
  p=b.new_page(viewport={'width':width,'height':700})
  errors=[];p.on('pageerror',lambda e:errors.append(str(e)))
  p.add_init_script("""window.events={};window.calls=[];window.__TAURI__={event:{listen:async(k,f)=>{events[k]=f;return ()=>{}}},core:{invoke:async(cmd,args)=>{calls.push(cmd);if(cmd==='install_update')return new Promise((ok,fail)=>{window.installOK=ok;window.installFail=fail;window.target=args.expectedVersion});if(cmd==='snapshot')return {entries:[],secrets:[],monitoring:false};if(cmd==='get_app_info')return new Promise((ok,fail)=>{window.historyOK=ok;window.histFail=fail});if(cmd==='check_update')return new Promise((ok,fail)=>{window.checkOK=ok;window.checkFail=fail});if(cmd==='pending_requests')return [];}}};""")
  p.add_init_script("localStorage.setItem('ky.lang',"+repr(lang)+")")
  p.goto('http://127.0.0.1:'+str(srv.server_port))
  p.evaluate("async lang=>(await import('./lang.js')).applyLanguage(lang,{persist:false,notifyBackend:false})",lang)
  p.wait_for_function("()=>Boolean(events['update-available'])")
  btn=p.locator('#about');lock=p.locator('#lock')
  title='更新日志' if lang=='zh-CN' else 'Release notes'
  expect(btn).to_have_accessible_name(title)
  expect(lock).to_have_accessible_name('隐藏屏幕内容' if lang=='zh-CN' else 'Hide screen content')
  for node in [btn,lock]:
   assert node.evaluate("e=>!e.matches('.nav-item') && getComputedStyle(e,'::after').content==='none' && e.querySelector('svg').getBoundingClientRect().width===15")
   assert node.locator('.action-caption').is_visible()==(width==1024)
  assert len({round(p.locator(s).bounding_box()['x']) for s in ['#about svg','#lock svg','[data-view="account"] .nav-icon','[data-view="settings"] .nav-icon']})==1
  p.evaluate("events['update-available']({payload:{version:'0.3.2'}})")
  expect(btn).to_have_accessible_name(title);expect(p.locator('#update-dot')).to_be_visible()
  btn.click();expect(p.get_by_role('dialog',name=title)).to_be_visible()
  expect(p.locator('#about-version')).to_have_text('v0.3.5');expect(p.locator('.current-notes')).to_contain_text('Ky' if width==640 else '首页')
  p.evaluate("histFail('offline')")
  expect(p.locator('.current-notes')).to_contain_text('Ky' if width==640 else '首页')
  p.locator('#check-update').click();expect(p.locator('#check-update')).to_be_disabled()
  p.evaluate("checkFail('offline')");expect(p.locator('#update-status')).to_contain_text('失败' if width==1024 else 'failed')
  p.locator('#check-update').click();p.evaluate('checkOK(null)');expect(p.locator('#update-dot')).to_be_hidden()
  expect(p.locator('#update-status')).to_contain_text('最新' if width==1024 else 'up to date')
  p.locator('#check-update').click();p.evaluate("checkOK('0.4.0')");expect(p.locator('#update-available-text')).to_contain_text('0.4.0')
  p.locator('#about-dialog .close').click();btn.click()
  p.evaluate("""historyOK({notes:[{version:'0.3.0',body:'![bad](https://invalid.test/x.png)',url:'https://invalid.test'}]})""")
  expect(p.locator('#release-notes a')).to_have_attribute('href','https://github.com/webkubor/kyvault/releases/tag/desktop-v0.3.0')
  assert p.locator('#release-notes img').count()==0 and '![' not in p.locator('#release-notes').inner_text()
  p.locator('#release-notes summary').click();p.locator('#release-notes a').press('Enter');assert p.evaluate("calls.includes('plugin:opener|open_url') && calls.filter(c=>c==='check_update').length===3")
  p.locator('#do-update').click();p.locator('#confirm-cancel').click()
  assert 'install_update' not in p.evaluate('calls')
  p.locator('#do-update').click();p.locator('#confirm-action').click()
  p.wait_for_function("()=>window.target==='0.4.0'")
  p.evaluate("events['update-progress']({payload:{version:'0.4.0',phase:'downloading',downloaded:250,total:500}})")
  expect(p.locator('#update-progress')).to_have_attribute('value','50')
  p.evaluate("installFail('network')");expect(p.locator('#do-update')).to_be_enabled()
  p.locator('#about-dialog .close').click(); btn.click()
  p.evaluate("histFail('offline')")
  p.locator('#release-notes button').click()
  p.evaluate("historyOK({notes:[]})")
  expect(p.locator('#release-notes button')).to_have_count(0)
  p.locator('#do-update').click();p.locator('#confirm-action').click()
  p.wait_for_function("()=>calls.filter(c=>c==='install_update').length===2")
  p.evaluate("events['update-progress']({payload:{version:'0.4.0',phase:'installed'}});installOK()")
  expect(p.locator('#update-status')).to_contain_text('重启' if width==1024 else 'Restarting')
  assert p.evaluate("localStorage.getItem('ky.installedVersion')")== '0.4.0'
  assert not errors,errors
  p.close()
 print('PASS widths 1024/640, zh/en, distinct icons, stable entry, offline notes, safe history, fresh check x3')
