// Writes the engine's global skeleton to a JSON file (input for bake_dance.mjs).
//   node tools/dump_skeleton.mjs skeleton.json
import fs from "fs";
import path from "path";
import { fileURLToPath, pathToFileURL } from "url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const realFetch = globalThis.fetch;
globalThis.fetch = async (u) => {                       // Node's fetch can't read file:// URLs
  const s = String(u);
  if (s.startsWith("file:")) return new Response(fs.readFileSync(fileURLToPath(s)), { status: 200 });
  return realFetch(u);
};
const { init, getSkeleton, getLastError } = await import(pathToFileURL(path.join(root, "packages/web/dist/index.js")).href);
await init({ partPackUrl: pathToFileURL(path.join(root, "rust-core/packs/essentials.afpp")).href, licenseKey: "" });
const sk = getSkeleton();
if (!sk) throw new Error("getSkeleton() failed: " + getLastError());
fs.writeFileSync(process.argv[2] || "skeleton.json", JSON.stringify(sk));
console.log("wrote", sk.length, "joints");
