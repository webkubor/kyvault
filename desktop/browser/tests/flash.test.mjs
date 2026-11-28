import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync} from 'node:fs';
import {attachCopy} from '../extension/listener.mjs';

const event = () => ({addListener(fn) { this.callback = fn; }, removeListener() {}});
const sent = [];
const settings = {'auto:example.org:8443': true};
globalThis.chrome = {
  runtime: {id:'ky', onInstalled:event(), onMessage:event(), async sendNativeMessage(...args) {
    sent.push(args); return {ok:true};
  }},
  extension:{inIncognitoContext:false}, action:{async setTitle(){}, async setBadgeText(){}},
  storage:{local:{async get(){ return settings; }}, onChanged:event()},
  permissions:{async contains(){return true;}, onRemoved:event()},
  tabs:{onUpdated:event()}, contextMenus:{onClicked:event()}
};
await import('../extension/worker.mjs');
const url = 'https://u:p@example.org:8443/doc?q=secret#here';
test('右键收藏保留原文并清理网址，跳过隐身和可编辑区域', () => {
  const info = {menuItemId:'ky-flash', pageUrl:url, selectionText:' 原文 '};
  const click = chrome.contextMenus.onClicked.callback;
  click(info,{id:1});
  assert.deepEqual(sent.pop(), ['com.kyvault.flash',{text:' 原文 ',url:'https://example.org:8443/doc'}]);
  click({...info,editable:true},{id:1});
  click(info,{incognito:true});
  click({...info,pageUrl:'file:///test'},{id:1});
  assert.equal(sent.length,0);
});
test('复制消息重新验证来源、顶层页面和精确端口',async () => {
  function copy(raw, frameId=0) {
    const sender = {id:'ky',url:raw,frameId,tab:{url:raw,id:1}};
    return new Promise(resolve => {
      if (!chrome.runtime.onMessage.callback({type:'copy',text:'A'},sender,resolve)) resolve(null);
    });
  }
  assert.deepEqual(await copy(url),{ok:true});
  assert.equal(sent.pop()[1].url,'https://example.org:8443/doc');
  for (const raw of ['https://example.org','https://sub.example.org:8443']) assert.deepEqual(await copy(raw),{ok:false});
  assert.equal(await copy(url,2),null);
  assert.equal(sent.length,0);
});
test('仅启用站点安装一次监听，输入区域和合成事件跳过，关闭即解除', async () => {
  const host = 'example.org:8443';
  globalThis.location = {host};
  let listener;
  let added = 0;
  globalThis.document = {addEventListener(_,fn){ listener=fn; added++; }, removeEventListener(){listener=null;}};
  globalThis.window = {getSelection(){return {toString:()=>'hello'};}};
  const messages = [];
  chrome.runtime.sendMessage = async msg => messages.push(msg);
  await attachCopy('other.org');
  assert.equal(added,0);
  await attachCopy(host);
  await attachCopy(host);
  assert.equal(added,1);
  listener({isTrusted:false});
  listener({isTrusted:true,composedPath:()=>[{closest:()=>true}]});
  assert.equal(messages.length,0);
  listener({isTrusted:true,composedPath:()=>[]});
  assert.deepEqual(messages,[{type:'copy',text:'hello'}]);
  chrome.storage.onChanged.callback({['auto:'+host]:{oldValue:true}},'local');
  assert.equal(listener,null);
});
test('固定扩展 ID、可选站点权限、禁用隐身', () => {
  const manifest = JSON.parse(readFileSync(new URL('../extension/manifest.json',import.meta.url)));
  assert.equal(manifest.incognito,'not_allowed');
  assert.equal(manifest.content_scripts,undefined);
  assert.equal(manifest.host_permissions,undefined);
  const id = createHash('sha256').update(Buffer.from(manifest.key,'base64')).digest('hex').slice(0,32)
    .replace(/./g,c=>String.fromCharCode(97+parseInt(c,16)));
  assert.equal(id,'fkgfhoglncnijiadgohpgnjllphjcibg');
});
