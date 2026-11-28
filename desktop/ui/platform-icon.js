// Only bundled assets are eligible. A custom vault name never becomes a URL.
const symbols = new Set([
  'bank', 'website', 'cmb', 'citic', 'stripe', 'bitiful',
  'alipay', 'alibabacloud', 'amap', 'anthropic', 'apple', 'cloudflare', 'discord',
  'bilibili', 'bluesky', 'douban', 'facebook', 'instagram', 'linkedin', 'mastodon',
  'sinaweibo', 'threads', 'tiktok', 'xiaohongshu', 'youtube',
  'feishu', 'github', 'gitlab', 'google', 'openai', 'telegram', 'wechat', 'x',
]);
const aliases = new Map([
  ['twitter', 'x'], ['weibo', 'sinaweibo'], ['微博', 'sinaweibo'], ['小红书', 'xiaohongshu'],
  ['豆瓣', 'douban'], ['哔哩哔哩', 'bilibili'], ['领英', 'linkedin'], ['推特', 'x'],
  ['aliyun', 'alibabacloud'], ['阿里云', 'alibabacloud'], ['支付宝', 'alipay'],
  ['高德地图', 'amap'], ['飞书', 'feishu'], ['苹果', 'apple'], ['微信', 'wechat'],
]);
const raster = new Set(['amap', 'feishu', 'cmb', 'citic', 'stripe', 'bitiful']);
const monochrome = new Set(['anthropic', 'apple', 'github', 'google', 'openai', 'threads', 'tiktok', 'x']);

const brands = [
  ['cmb', /招商银行|招行|(?:^|[^a-z0-9])(?:cmb|cmbchina)(?:$|[^a-z0-9])/i],
  ['citic', /中信银行|(?:^|[^a-z0-9])(?:citic|citicbank)(?:$|[^a-z0-9])/i],
  ['stripe', /(?:^|[^a-z0-9])stripe(?:$|[^a-z0-9])/i],
  ['bitiful', /缤纷云|(?:^|[^a-z0-9])bitiful(?:$|[^a-z0-9])/i],
];
export function credentialSymbol(entry) {
  const id = String(entry.platform || '').trim().toLowerCase();
  const bank = /^(bank|banking|银行)$/.test(id);
  const direct = aliases.get(id) || id;
  if (!bank && symbols.has(direct)) return direct;
  const hints = [id, entry.name, entry.reference, ...(entry.aliases || [])].filter(Boolean).join(' ');
  for (const [symbol, pattern] of brands) if (pattern.test(hints)) return symbol;
  if (bank) return 'bank';
  if (/^(blog|website|web|site|博客|网站)$/.test(id) || /^(?:https?:\/\/)?(?:[a-z0-9-]+\.)+[a-z]{2,}\/?$/i.test(id)) return 'website';
  return id;
}

export function platformMark(id) {
  const label = String(id || '').trim();
  const key = label.toLowerCase();
  const symbol = aliases.get(key) || key;
  const mark = document.createElement('span');
  mark.className = 'platform-mark';
  mark.setAttribute('aria-hidden', 'true');
  if (symbols.has(symbol)) {
    const image = document.createElement('img');
    image.src = new URL(`./icons/platforms/${symbol}.${raster.has(symbol) ? 'png' : 'svg'}`, import.meta.url).href;
    image.alt = '';
    image.draggable = false;
    if (monochrome.has(symbol)) image.className = 'mono-mark';
    mark.append(image);
  } else {
    mark.textContent = Array.from(label).slice(0, 2).join('').toUpperCase() || '·';
  }
  return mark;
}
