// Only bundled assets are eligible. A custom vault name never becomes a URL.
const symbols = new Set([
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
const raster = new Set(['amap', 'feishu']);
const monochrome = new Set(['anthropic', 'apple', 'github', 'google', 'openai', 'threads', 'tiktok', 'x']);

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
