import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";

const assets = "static/assets";
const fontAwesome = "node_modules/@fortawesome/fontawesome-free";

await mkdir(`${assets}/vendor/fontawesome/css`, { recursive: true });
await mkdir(`${assets}/vendor/fontawesome/webfonts`, { recursive: true });

const baseCss = await readFile(`${fontAwesome}/css/fontawesome.min.css`, "utf8");
const solidCss = await readFile(`${fontAwesome}/css/solid.min.css`, "utf8");
const brandsCss = await readFile(`${fontAwesome}/css/brands.min.css`, "utf8");
await writeFile(
  `${assets}/vendor/fontawesome/css/all.min.css`,
  `${baseCss}\n${solidCss}\n${brandsCss}`,
);
await copyFile(
  `${fontAwesome}/webfonts/fa-solid-900.woff2`,
  `${assets}/vendor/fontawesome/webfonts/fa-solid-900.woff2`,
);
await copyFile(
  `${fontAwesome}/webfonts/fa-brands-400.woff2`,
  `${assets}/vendor/fontawesome/webfonts/fa-brands-400.woff2`,
);
await copyFile(
  `${fontAwesome}/LICENSE.txt`,
  `${assets}/vendor/fontawesome/LICENSE.txt`,
);

await copyFile("node_modules/htmx.org/dist/htmx.min.js", `${assets}/htmx.min.js`);
await copyFile("node_modules/htmx.org/LICENSE", `${assets}/vendor/htmx.LICENSE.txt`);

console.log(`Vendored browser assets into ${assets}`);
