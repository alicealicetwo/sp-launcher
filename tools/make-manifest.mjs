#!/usr/bin/env node
/**
 * Build a launcher manifest from an existing game install.
 *
 * Walks the install folder, hashes every file with SHA-256, and writes the
 * manifest.json the launcher expects. Run it once after DepotDownloader has
 * produced the files, upload the folder and the manifest to your host, and
 * point the launcher's manifest URL at it.
 *
 *   node tools/make-manifest.mjs \
 *     --dir "C:/Games/SUPER PEOPLE" \
 *     --base-url "https://files.example.com/sp/1.3.0.0" \
 *     --version 1.3.0.0 \
 *     --out manifest.json
 *
 * For a quick smoke test, --limit and --max-size keep the run to a handful of
 * small files so a full pass takes seconds instead of an evening:
 *
 *   node tools/make-manifest.mjs --dir ... --base-url http://localhost:8080 \
 *     --limit 20 --max-size 50MB --out manifest.test.json
 *
 * Hashing 28 GB takes a few minutes; it runs several files at a time.
 */

import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { readdir, stat, writeFile } from "node:fs/promises";
import path from "node:path";

const CONCURRENCY = 8;

/** Files that should never ship to players. */
const SKIP_NAMES = new Set([
  "manifest.json",
  ".depotdownloader",
  "desktop.ini",
  "thumbs.db",
  ".ds_store",
]);
const SKIP_EXTS = new Set([".part", ".tmp", ".log", ".bak"]);

function parseArgs(argv) {
  const out = {};
  for (let i = 2; i < argv.length; i += 2) {
    const key = argv[i].replace(/^--/, "");
    out[key] = argv[i + 1];
  }
  return out;
}

async function* walk(root, rel = "") {
  const entries = await readdir(path.join(root, rel), { withFileTypes: true });
  for (const entry of entries) {
    const childRel = rel ? `${rel}/${entry.name}` : entry.name;
    if (entry.isDirectory()) {
      yield* walk(root, childRel);
    } else if (entry.isFile()) {
      const lower = entry.name.toLowerCase();
      if (SKIP_NAMES.has(lower) || SKIP_EXTS.has(path.extname(lower))) continue;
      yield childRel;
    }
    // symlinks and specials are skipped: a manifest should describe bytes
  }
}

function sha256(file) {
  return new Promise((resolve, reject) => {
    const hash = createHash("sha256");
    const stream = createReadStream(file, { highWaterMark: 4 * 1024 * 1024 });
    stream.on("data", (c) => hash.update(c));
    stream.on("error", reject);
    stream.on("end", () => resolve(hash.digest("hex")));
  });
}

function human(n) {
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(units.length - 1, Math.floor(Math.log(n || 1) / Math.log(1024)));
  return `${(n / 1024 ** i).toFixed(i === 0 ? 0 : 2)} ${units[i]}`;
}

/** URL-encode each path segment, leaving the separators intact. */
function encodePath(rel) {
  return rel.split("/").map(encodeURIComponent).join("/");
}

const args = parseArgs(process.argv);
const dir = args.dir;
const baseUrl = (args["base-url"] ?? "").replace(/\/+$/, "");
const version = args.version ?? "0.0.0";
const out = args.out ?? "manifest.json";
const limit = args.limit ? Number(args.limit) : Infinity;
const maxSize = parseSize(args["max-size"]);

/** "50MB" / "2GB" / plain bytes -> bytes. */
function parseSize(s) {
  if (!s) return Infinity;
  const m = /^(\d+(?:\.\d+)?)\s*(b|kb|mb|gb)?$/i.exec(String(s).trim());
  if (!m) return Infinity;
  const mult = { b: 1, kb: 1024, mb: 1024 ** 2, gb: 1024 ** 3 }[(m[2] ?? "b").toLowerCase()];
  return Number(m[1]) * mult;
}

if (!dir || !baseUrl) {
  console.error("usage: make-manifest.mjs --dir <install folder> --base-url <https://host/path> [--version X] [--out manifest.json]");
  process.exit(1);
}

let rels = [];
for await (const rel of walk(dir)) rels.push(rel);
rels.sort();

const foundAll = rels.length;
if (maxSize !== Infinity) {
  const kept = [];
  for (const rel of rels) {
    const info = await stat(path.join(dir, rel));
    if (info.size <= maxSize) kept.push(rel);
  }
  rels = kept;
}
if (rels.length > limit) rels = rels.slice(0, limit);

if (rels.length !== foundAll) {
  console.log(`found ${foundAll} files, using ${rels.length} (test subset)`);
} else {
  console.log(`found ${rels.length} files`);
}
console.log(`hashing with ${CONCURRENCY} workers\u2026`);

const files = new Array(rels.length);
let done = 0;
let totalBytes = 0;
let cursor = 0;

async function worker() {
  for (;;) {
    const i = cursor++;
    if (i >= rels.length) return;
    const rel = rels[i];
    const abs = path.join(dir, rel);
    const [info, digest] = await Promise.all([stat(abs), sha256(abs)]);
    files[i] = { path: rel, url: `${baseUrl}/${encodePath(rel)}`, size: info.size, sha256: digest };
    totalBytes += info.size;
    done += 1;
    if (done % 25 === 0 || done === rels.length) {
      process.stdout.write(`\r  ${done}/${rels.length}  ${human(totalBytes)}   `);
    }
  }
}

await Promise.all(Array.from({ length: CONCURRENCY }, worker));
process.stdout.write("\n");

await writeFile(out, JSON.stringify({ version, files }, null, 2));
console.log(`wrote ${out}`);
console.log(`  ${files.length} files, ${human(totalBytes)} total`);
console.log(`  players will fetch from ${baseUrl}/...`);
