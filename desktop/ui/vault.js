// Display vocabulary only. Stored references remain the canonical identifiers.
import { bankNames, credentialSymbol } from './platform-icon.js';
const platforms = {
  bank: ['银行','Bank'], citic: ['中信银行','China CITIC Bank'], cmb: ['招商银行','China Merchants Bank'],
  stripe: ['Stripe','Stripe'], bitiful: ['缤纷云','Bitiful'],
  alipay: ['支付宝', 'Alipay'], apple: ['苹果', 'Apple'], amap: ['高德地图', 'Amap'],
  aliyun: ['阿里云', 'Aliyun'], feishu: ['飞书', 'Feishu'], github: ['GitHub', 'GitHub'],
  openai: ['OpenAI', 'OpenAI'], gitlab: ['GitLab', 'GitLab'], 'accounts-legacy': ['旧账号', 'Legacy accounts'],
};
const purposes = [
  ['app private key', '应用私钥'], ['app public key', '应用公钥'], ['private key', '私钥'],
  ['public key', '公钥'], ['api key', 'API 密钥'], ['aes key', 'AES 密钥'],
  ['app id', '应用 ID'], ['apple id', 'Apple ID'], ['password', '密码'],
  ['backup', '备份'], ['token', '令牌'], ['pin', 'PIN 码'],
];

export function platformLabel(id, lang) {
  return platforms[id]?.[lang === 'en' ? 1 : 0] || bankNames.get(id) || id;
}

function describe(entry, english) {
  const symbol = credentialSymbol(entry);
  const brand = (bankNames.has(symbol) || ['stripe','bitiful'].includes(symbol)) ? symbol : entry.platform;
  const platform = platformLabel(brand, english ? 'en' : 'zh-CN');
  const raw = entry.name || entry.reference.split('/').slice(3).join('/');
  const name = raw.startsWith(entry.platform + '-') ? raw.slice(entry.platform.length + 1) : raw;
  let purpose = name.replace(/[-_/]+/g, ' ').trim();
  if (brand !== entry.platform) purpose = purpose.replace(new RegExp('^' + brand + '(?: |$)', 'i'), '');
  if (!english) {
    if (purpose.toLowerCase() === 'card') purpose = '银行卡';
    for (const [term, label] of purposes) {
      purpose = purpose.replace(new RegExp('\\b' + term + '\\b', 'gi'), label);
    }
  }
  return [platform, purpose].filter(Boolean).join(' · ');
}

export function credentialTitle(entry, lang) {
  return describe(entry, lang === 'en');
}

export function matchesCredential(entry, query) {
  const fields = [entry.reference, entry.account, entry.kind, ...(entry.aliases || []),
    describe(entry, false), describe(entry, true)].join(' ').toLowerCase();
  return query.toLowerCase().trim().split(/\s+/).every(word => fields.includes(word));
}
