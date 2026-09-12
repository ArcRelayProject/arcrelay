import { readdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

const root = path.resolve(process.env.ARCRELAY_SCREENSHOT_DIR ?? "visual-artifacts/actual");

async function files(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  return (await Promise.all(entries.map((entry) => entry.isDirectory()
    ? files(path.join(directory, entry.name))
    : [path.join(directory, entry.name)]))).flat();
}

const images = (await files(root)).filter((file) => file.endsWith(".png")).sort();
const cards = images.map((file) => {
  const relative = path.relative(root, file);
  return `<article><img loading="lazy" src="${relative.split(path.sep).join("/")}" alt="${relative}"><p>${relative}</p></article>`;
}).join("\n");
const html = `<!doctype html><meta charset="utf-8"><title>ArcRelay desktop screenshots</title>
<style>body{margin:0;padding:24px;font:13px system-ui;background:#111;color:#eee}header{position:sticky;top:0;padding:12px;background:#111;z-index:1}main{display:grid;grid-template-columns:repeat(auto-fill,minmax(320px,1fr));gap:18px}article{background:#202126;border-radius:12px;padding:10px}img{width:100%;height:260px;object-fit:contain;background:#090909}p{overflow-wrap:anywhere}</style>
<header><strong>ArcRelay desktop screenshots</strong> · ${images.length} images</header><main>${cards}</main>`;
await writeFile(path.join(root, "report.html"), html);
const metadata = await Promise.all(images.map((image) => readFile(image.replace(/\.png$/, ".json"), "utf8").then(JSON.parse)));
await writeFile(path.join(root, "manifest.json"), `${JSON.stringify(metadata, null, 2)}\n`);
console.log(`${images.length} screenshots indexed at ${path.join(root, "report.html")}`);
