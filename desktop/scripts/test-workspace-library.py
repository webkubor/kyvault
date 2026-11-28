"""Exercise the main window with an in-memory bridge. Optional PNGs: KY_LIBRARY_PROOF_DIR."""
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from threading import Thread
import os
import unittest

from playwright.sync_api import expect, sync_playwright


class LibraryTest(unittest.TestCase):
    def test_library(self):
        ui = Path(__file__).resolve().parents[1] / 'ui'
        with ThreadingHTTPServer(('127.0.0.1', 0), partial(SimpleHTTPRequestHandler, directory=ui)) as server:
            Thread(target=server.serve_forever, daemon=True).start()
            self.addCleanup(server.shutdown)
            with sync_playwright() as pw:
                browser = pw.chromium.launch(channel='chrome')
                page = browser.new_page(viewport={'width': 1024, 'height': 680}, reduced_motion='reduce', color_scheme='light')
                page.add_init_script("""
                    const docs = {private:[], drafts:[], shares:[], feedback:[]};
                    window.fixtureDocs=docs;
                    window.__TAURI__ = {event:{listen:async()=>()=>{}}, core:{invoke:async(cmd,a)=>{
                      if(cmd==='snapshot') return {entries:[],secrets:[],monitoring:false,lang:'zh-CN'};
                      if(cmd==='pending_requests') return [];
                      if(cmd==='workspace_list') return docs[a.kind].map(({id,title})=>({id,title}));
                      if(cmd==='workspace_read') return docs[a.kind].find(r=>r.id===a.id);
                      if(cmd==='workspace_save') {
                        const rows=docs[a.kind], id=a.id || String(rows.length+1);
                        const i=rows.findIndex(r=>r.id===id);
                        rows[i<0?rows.length:i]={...a,id}; return id;
                      }
                    }}};
                """)
                page.goto(f'http://127.0.0.1:{server.server_port}/')
                wk = page.locator('#workspace')
                def click(name):
                    wk.get_by_role('button', name=name, exact=True).click()
                def shot(name):
                    if dest := os.environ.get('KY_LIBRARY_PROOF_DIR'):
                        page.screenshot(path=str(Path(dest) / (name + '.png')))
                def nav(kind):
                    page.locator(f'.nav-item[data-view="{kind}"]').click()

                def active(kind):
                    expect(page.locator(f'.nav-item[data-view="{kind}"]')).to_have_attribute('aria-current', 'page')

                nav('drafts')
                expect(wk.get_by_text('还没有草稿。', exact=False)).to_contain_text('用作想法草稿')
                click('写草稿')
                expect(wk.get_by_label('标题', exact=True)).to_have_value('')
                click('取消')
                nav('private')
                expect(wk.get_by_text('左击菜单栏', exact=False)).to_contain_text('⌥⌘N')
                query = wk.get_by_role('searchbox')
                query.fill('不存在的标题')
                click('清除搜索')
                expect(query).to_be_focused()
                title = '读书会开场想法'
                body = '从一段旅途见闻开始。\n\n仅供自己参考的笔记。'
                click('添加收藏')
                wk.get_by_label('标题', exact=True).fill(title)
                wk.get_by_label('正文', exact=True).fill(body)
                wk.locator('input[type=file]').set_input_files(str(ui / 'logo.png'))
                expect(wk.locator('.wk-image')).to_be_visible()
                click('保存')
                click('返回收藏列表')
                query.fill('读书会')
                shot('private')
                click(title)
                shot('private-detail')
                click('用作想法草稿')
                active('drafts')
                expect(wk.get_by_label('正文', exact=True)).to_have_value(body)
                expect(wk.locator('.wk-image')).to_be_visible()
                self.assertEqual(page.evaluate('fixtureDocs.drafts.length'), 0)
                wk.get_by_label('标题', exact=True).fill('取消这次修改')
                click('取消')
                click('放弃更改')
                active('private')
                expect(wk.get_by_role('heading', name=title)).to_be_visible()
                self.assertEqual(page.evaluate('fixtureDocs.drafts.length'), 0)
                click('用作想法草稿')
                wk.get_by_label('标题', exact=True).fill('修订 · ' + title)
                shot('draft-editor')
                click('保存')
                active('drafts')
                click('返回草稿列表')
                query.fill('读书会')
                shot('drafts')
                click('修订 · ' + title)
                click('继续编辑')
                wk.get_by_label('正文', exact=True).fill(body.replace('旅途见闻', '书店见闻'))
                click('保存')
                self.assertTrue(page.evaluate('fixtureDocs.drafts.length===1 && fixtureDocs.drafts[0].image===fixtureDocs.private[0].image'))
                click('选择分享内容')
                boxes = wk.get_by_role('checkbox')
                expect(boxes).to_have_count(3)
                expect(boxes.first).not_to_be_checked()
                expect(boxes.nth(1)).not_to_be_checked()
                expect(boxes.last).not_to_be_checked()
                boxes.first.check()
                click('预览分享')
                expect(wk.get_by_text('仅供自己参考的笔记。')).to_have_count(0)
                expect(wk.get_by_text('从一段书店见闻开始。')).to_be_visible()
                expect(wk.locator('.wk-share-photo')).to_have_count(0)
                shot('share-preview')
                click('保存此分享版本')
                expect(page.locator('.nav-item[data-view="shares"]')).to_have_attribute('aria-current','page')
                nav('private')
                click(title)
                expect(wk.get_by_text(body)).to_be_visible()
                expect(wk.locator('.wk-image')).to_be_visible()
                self.assertEqual(page.evaluate('fixtureDocs.private.length'), 1)
                page.set_viewport_size({'width':680,'height':480})
                page.emulate_media(color_scheme='dark')
                page.locator('.nav-item[data-view="drafts"]').click()
                wk.get_by_role('searchbox').fill('书店')
                expect(wk.get_by_role('button', name='清除搜索')).to_be_visible()
                shot('drafts-no-results')
                click('清除搜索')
                expect(wk.get_by_role('button', name='修订 · 读书会开场想法')).to_be_visible()
                browser.close()


if __name__ == '__main__':
    unittest.main()
