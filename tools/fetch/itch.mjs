// Downloads the free tier of an itch.io pack with headless Chrome: node itch.mjs <slug> <outdir>
import { chromium } from 'playwright-core';
import fs from 'node:fs';
const [slug, out] = process.argv.slice(2);
fs.mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
const ctx = await browser.newContext({ acceptDownloads: true });
const page = await ctx.newPage();
await page.goto(`https://quaternius.itch.io/${slug}`, { waitUntil: 'domcontentloaded' });
await page.click('.buy_btn, .download_btn, a:has-text("Download Now")');
const nothanks = page.locator('a:has-text("No thanks, just take me to the downloads")');
await nothanks.waitFor({ timeout: 20000 });
await nothanks.click();
await page.waitForSelector('.upload .download_btn', { timeout: 30000 });
const buttons = await page.locator('.upload .download_btn').all();
for (const b of buttons) {
  const [dl] = await Promise.all([page.waitForEvent('download', { timeout: 600000 }), b.click()]);
  const name = dl.suggestedFilename();
  await dl.saveAs(`${out}/${name}`);
  console.log('saved', name);
}
await browser.close();
