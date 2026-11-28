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
  p.add_init_script("""window.elapsed=100;Object.defineProperty(performance,"now",{value:()=>window.elapsed});window.events={};window.calls=[];window.__TAURI__={event:{listen:async(k,f)=>{events[k]=f;return ()=>{}}},core:{invoke:async(cmd,args)=>{calls.push(cmd);if(cmd==='install_update')return new Promise((ok,fail)=>{window.installOK=ok;window.installFail=fail;window.target=args.expectedVersion});if(cmd==='snapshot')return {entries:[],secrets:[],monitoring:false};if(cmd==='check_update')return new Promise((ok,fail)=>{window.checkOK=ok;window.checkFail=fail});if(cmd==='workspace_list'||cmd==='pending_requests')return [];}}};""")
  p.add_init_script("localStorage.setItem('ky.lang',"+repr(lang)+")")
  p.goto('http://127.0.0.1:'+str(srv.server_port))
  p.evaluate("async lang=>(await import('./lang.js')).applyLanguage(lang,{persist:false,notifyBackend:false})",lang)
  p.wait_for_function("()=>Boolean(window.checkOK)")
  btn=p.locator('#about');lock=p.locator('#lock')
  title='更新日志' if lang=='zh-CN' else 'Release notes'
  expect(btn).to_have_accessible_name(title)
  expect(lock).to_have_accessible_name('隐藏屏幕内容' if lang=='zh-CN' else 'Hide screen content')
  for node in [btn,lock]:
   assert node.evaluate("e=>!e.matches('.nav-item') && getComputedStyle(e,'::after').content==='none' && e.querySelector('svg').getBoundingClientRect().width===15")
   assert node.locator('.action-caption').is_visible()==(width==1024)
  assert len({round(p.locator(s).bounding_box()['x']) for s in ['#about svg','#lock svg','[data-view="account"] .nav-icon','[data-view="settings"] .nav-icon']})==1
  btn.click()
  expect(p.locator('#check-update')).to_be_disabled()
  p.locator('#about-dialog .close').click()
  assert p.evaluate("calls.filter(c=>c==='check_update').length") == 1
  p.evaluate("checkOK({version:'0.4.0',notes:'Fixture notes <img src=x>'})")
  notice=p.locator('#update-notice')
  expect(notice).to_contain_text('查看更新内容' if width==1024 else 'View release notes')
  expect(btn).to_have_accessible_name(title);expect(p.locator('#update-dot')).to_be_visible()
  notice.press('Enter');expect(p.get_by_role('dialog',name=title)).to_be_visible()
  expect(p.locator('#update-notes')).to_have_text('Fixture notes <img src=x>')
  assert p.locator('#update-notes img').count()==0
  expect(p.locator('#about-version')).to_have_text('v0.3.8');expect(p.locator('.current-notes')).to_contain_text('microphone or speech recognition' if width==640 else '麦克风或语音识别')
  p.locator('#check-update').click();expect(p.locator('#check-update')).to_be_disabled()
  p.evaluate("checkFail('offline')");expect(p.locator('#update-status')).to_contain_text('失败' if width==1024 else 'failed')
  expect(notice).to_be_visible()
  expect(p.locator('#update-notes')).to_have_text('Fixture notes <img src=x>')
  p.locator('#check-update').click();p.evaluate('checkOK(null)');expect(p.locator('#update-dot')).to_be_hidden()
  expect(p.locator('#update-status')).to_contain_text('最新' if width==1024 else 'up to date')
  p.locator('#check-update').click();p.evaluate("checkOK({version:'0.4.0',notes:null})");expect(p.locator('#update-available-text')).to_contain_text('0.4.0')
  expect(p.locator('#update-notes')).to_contain_text('详细说明' if width==1024 else 'no detailed notes')
  p.locator('#about-dialog .close').click();btn.click()
  link=p.locator('#about-dialog a')
  expect(link).to_have_count(1)
  expect(link).to_have_attribute('href','https://github.com/webkubor/kyvault/releases')
  expect(p.locator('#about-dialog details')).to_have_count(0)
  link.press('Enter')
  assert p.evaluate("calls.includes('plugin:opener|open_url') && !calls.includes('get_app_info')")
  p.locator('#do-update').click();p.locator('#confirm-cancel').click()
  assert 'install_update' not in p.evaluate('calls')
  p.locator('#do-update').click();p.locator('#confirm-action').click()
  p.wait_for_function("()=>window.target==='0.4.0'")
  p.evaluate("events['update-progress']({payload:{version:'0.4.0',phase:'downloading',downloaded:250,total:500}})")
  expect(p.locator('#update-progress')).to_have_attribute('value','50')
  p.evaluate("installFail('network')");expect(p.locator('#do-update')).to_be_enabled()
  p.locator('#about-dialog .close').click(); btn.click()
  p.locator('#do-update').click();p.locator('#confirm-action').click()
  p.wait_for_function("()=>calls.filter(c=>c==='install_update').length===2")
  p.evaluate("events['update-progress']({payload:{version:'0.4.0',phase:'installed'}});installOK()")
  expect(p.locator('#update-status')).to_contain_text('重启' if width==1024 else 'Restarting')
  assert p.evaluate("localStorage.getItem('ky.installedVersion')")== '0.4.0'
  p.reload()
  p.wait_for_function("()=>Boolean(window.checkOK)")
  p.evaluate("checkOK(null)")
  p.wait_for_function("()=>!document.querySelector('#check-update').disabled")
  resume = """() => { events['tauri://focus']({}); window.dispatchEvent(new Event('focus'));
                       document.dispatchEvent(new Event('visibilitychange')); }"""
  checks = "calls.filter(c=>c==='check_update').length"
  p.evaluate(resume)
  assert p.evaluate(checks)==1
  p.evaluate('window.elapsed += 15*60*1000')
  p.evaluate(resume)
  assert p.evaluate(checks)==2
  p.evaluate("checkOK({version:'0.4.1',notes:'fixture'})")
  expect(notice).to_contain_text('0.4.1')
  p.evaluate(resume)
  assert p.evaluate(checks)==2
  notice.click()
  p.locator('#check-update').click()
  assert p.evaluate(checks)==3
  p.evaluate("checkOK({version:'0.4.1',notes:'fixture'})")
  p.locator('#about-dialog .close').click()
  p.locator('.nav-item[data-view="private"]').click()
  p.locator('#workspace .wk-actions button.primary').click()
  p.locator('#workspace textarea').fill('fixture draft')
  notice.click()
  p.locator('#do-update').click()
  expect(p.locator('#update-status')).to_contain_text('保存' if lang=='zh-CN' else 'Save')
  assert 'install_update' not in p.evaluate('calls')
  assert not errors,errors
  p.close()
 print('PASS widths 1024/640, zh/en, distinct icons, stable entry, offline notes, single GitHub history link, startup and throttled resume, manual bypass, preview fallback, unsaved guard, install retry and restart')
