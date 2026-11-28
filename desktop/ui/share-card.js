// Work entirely in memory. Only pass the selected share snapshot into this module.
import { validImage } from './workspace-files.js';

const WIDTH = 1080;
const MAX_HEIGHT = 4096;
const EDGE = 72;
const CONTENT = WIDTH - EDGE * 2;
const FONT = '-apple-system, BlinkMacSystemFont, "PingFang SC", sans-serif';

function linesFor(ctx, text, locale, lineHeight) {
  const result = [];
  const split = typeof Intl.Segmenter === 'function'
    ? new Intl.Segmenter(locale, {granularity:'grapheme'}) : null;
  function add(line) {
    result.push(line.trimEnd());
    if (result.length * lineHeight > MAX_HEIGHT) throw Error('cardTooLong');
  }
  for (const paragraph of text.replace(/\r\n?/g, '\n').split('\n')) {
    let line = '';
    // Break on word boundaries first; URLs and uninterrupted CJK can break
    // within a word, while emoji and combining characters stay together.
    for (const token of paragraph.match(/\s+|\S+/gu) || []) {
      if (ctx.measureText(line + token).width <= CONTENT) { line += token; continue; }
      if (line.trim()) { add(line); line = ''; }
      if (!token.trim()) continue;
      if (ctx.measureText(token).width <= CONTENT) { line = token; continue; }
      const parts = split ? Array.from(split.segment(token), item => item.segment) : Array.from(token);
      for (const part of parts) {
        if (line && ctx.measureText(line + part).width > CONTENT) { add(line); line = ''; }
        line += part;
      }
    }
    add(line);
  }
  return result;
}

export async function renderShareCard(record, lang = 'zh-CN') {
  const title = String(record.title || '').trim();
  const body = String(record.body || '').trim();
  if (!title || (!body && !record.image)) throw Error('invalid');
  const canvas = document.createElement('canvas');
  const ctx = canvas.getContext('2d', {alpha:false});
  if (!ctx) throw Error('cardRender');
  let picture;
  try {
    if (record.image) {
      if (!validImage(record.image)) throw Error('badImage');
      picture = new Image();
      picture.src = record.image;
      await picture.decode();
    }
    const headingFont = '650 54px ' + FONT;
    const bodyFont = '400 34px ' + FONT;
    ctx.font = headingFont;
    const heading = linesFor(ctx, title, lang, 72);
    ctx.font = bodyFont;
    const paragraphs = body ? linesFor(ctx, body, lang, 52) : [];
    const imageScale = picture ? Math.min(CONTENT / picture.naturalWidth, 1100 / picture.naturalHeight) : 0;
    const imageHeight = picture ? Math.round(picture.naturalHeight * imageScale) : 0;
    const firstLine = 113;
    const bodyTop = firstLine + heading.length * 72 + 20;
    const imageTop = bodyTop + paragraphs.length * 52 + (paragraphs.length ? 24 : 0);
    const height = Math.max(520, imageTop + imageHeight + EDGE);
    if (height > MAX_HEIGHT) throw Error('cardTooLong');
    canvas.width = WIDTH;
    canvas.height = height;
    ctx.fillStyle = '#edf4ee';
    ctx.fillRect(0, 0, WIDTH, height);
    ctx.fillStyle = '#fbfcf8';
    ctx.fillRect(20,20,WIDTH-40,height-40);
    ctx.fillStyle = '#23745a';
    ctx.fillRect(EDGE,60,64,6);
    ctx.textBaseline = 'top';
    ctx.font = headingFont;
    ctx.fillStyle = '#172f25';
    heading.forEach((line,i) => ctx.fillText(line, EDGE, firstLine + i*72));
    ctx.font = bodyFont;
    ctx.fillStyle = '#32483e';
    paragraphs.forEach((line,i) => ctx.fillText(line, EDGE, bodyTop + i*52));
    if (picture) {
      const width = Math.round(picture.naturalWidth * imageScale);
      ctx.drawImage(picture, (WIDTH-width)/2, imageTop, width, imageHeight);
    }
    return await new Promise((resolve,reject) => canvas.toBlob(blob => {
      if (!blob) reject(Error('cardRender'));
      else resolve(blob);
    }, 'image/png'));
  } finally {
    picture?.removeAttribute('src');
    canvas.width = canvas.height = 0;
  }
}
