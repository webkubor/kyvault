import assert from 'node:assert/strict';
import { credentialTitle, matchesCredential } from '../ui/vault.js';

const old = { reference: 'secret://alipay/alipay-app-private-key', platform: 'alipay' };
assert.equal(credentialTitle(old, 'zh-CN'), '支付宝 · 应用私钥');
assert.equal(credentialTitle(old, 'en'), 'Alipay · app private key');
for (const query of ['支付宝 私钥', '私钥 ALIPAY', old.reference]) {
  assert.ok(matchesCredential(old, query));
}
assert.equal(matchesCredential(old, '支付宝 公钥'), false);
const apple = { platform: 'apple', name: 'work-apple-id-password', reference: 'secret://apple/work-apple-id-password',
  account: 'work@example.test', aliases: ['team-login'] };
for (const query of ['Apple 密码', '苹果 密码', 'work@example.test', 'team-login']) {
  assert.ok(matchesCredential(apple, query));
}
assert.equal(credentialTitle({ platform: 'custom', name: 'prod/db-user' }, 'zh-CN'), 'custom · prod db user');
console.log('旧引用、中英文名称、混合搜索、账号、别名及自定义平台验证通过');

const {credentialSymbol} = await import('../ui/platform-icon.js');
for (const [entry, symbol] of [
  [{platform:'bank', reference:'secret://bank/citic-card'},'citic'],
  [{platform:'bank', name:'cmb-card'},'cmb'],
  [{platform:'bank', name:'日常使用',aliases:['招商银行']},'cmb'],
  [{platform:'bank', name:'stripe-password'},'stripe'],
  [{platform:'bank', name:'uncmbrelated'},'bank'],
  [{platform:'bitiful'},'bitiful'], [{platform:'blog'},'website'],
  [{platform:'apple',name:'cmb-card'},'apple'],
]) assert.equal(credentialSymbol(entry),symbol);

for (const [name,expected] of [['cmbc-card','cmbc'],['农业银行','abc'],['建行杭州支行','ccb'],['ICBC-password','icbc'],['银行账户','bank']]) {
  assert.equal(credentialSymbol({platform:'bank',name}),expected);
}
