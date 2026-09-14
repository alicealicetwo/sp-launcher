#!/usr/bin/env node
/**
 * Dev file server for testing the launcher's downloader.
 *
 * Serves a folder over HTTP with proper range support, so resume behaves the
 * same as it will against real hosting. Kill it mid-download and restart it to
 * test resume; that is the whole point of having it.
 *
 *   node tools/serve-files.mjs --dir "C:/Games/SUPER PEOPLE" --port 8080
 *
 * Options:
 *   --no-range     refuse range requests (always 200), to check the launcher's
 *                  fallback path when a host does not support them
 *   --throttle N   cap at N MB/s, so a local test takes long enough to cancel
 */

import { createReadStream, statSync } from "node:fs";
import { createServer } from "node:http";
import path from "node:path";
import { pipeline } from "node:stream/promises";
import { Transform } from "node:stream";

function parseArgs(argv) {
  const out = {};
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (!a.startsWith("--")) continue;
    const key = a.replace(/^--/, "");
    const next = argv[i + 1];
    if (next && !next.startsWith("--")) { out[key] = next; i++; } else { out[key] = true; }
  }
  return out;
}

const args = parseArgs(process.argv);
const root = path.resolve(args.dir ?? ".");
const port = Number(args.port ?? 8080);
const allowRange = !args["no-range"];
const throttle = args.throttle ? Number(args.throttle) * 1024 * 1024 : 0;

const TYPES = {
  ".json": "application/json",
  ".pak": "application/octet-stream",
  ".exe": "application/octet-stream",
  ".ini": "text/plain; charset=utf-8",
  ".dll": "application/octet-stream",
};

/** Simple byte-rate limiter so a LAN test is slow enough to interrupt. */
function limiter(bytesPerSec) {
  let budget = bytesPerSec;
  const timer = setInterval(() => { budget = bytesPerSec; }, 1000);
  timer.unref();
  return new Transform({
    async transform(chunk, _enc, cb) {
      let offset = 0;
      while (offset < chunk.length) {
        if (budget <= 0) {
          await new Promise((r) => setTimeout(r, 60));
          continue;
        }
        const take = Math.min(budget, chunk.length - offset);
        this.push(chunk.subarray(offset, offset + take));
        offset += take;
        budget -= take;
      }
      cb();
    },
  });
}

const server = createServer(async (req, res) => {
  // Decode, then confine to the root: a request for ../../etc/passwd must not
  // escape, exactly as the launcher refuses such paths in a manifest.
  let rel;
  try {
    rel = decodeURIComponent(new URL(req.url, "http://x").pathname);
  } catch {
    res.writeHead(400).end("bad path");
    return;
  }
  const abs = path.join(root, rel);
  if (!abs.startsWith(root)) {
    res.writeHead(403).end("forbidden");
    return;
  }

  let info;
  try {
    info = statSync(abs);
    if (!info.isFile()) throw new Error("not a file");
  } catch {
    console.log(`404 ${rel}`);
    res.writeHead(404).end("not found");
    return;
  }

  const type = TYPES[path.extname(abs).toLowerCase()] ?? "application/octet-stream";
  const range = allowRange ? req.headers.range : undefined;

  if (range) {
    const m = /^bytes=(\d*)-(\d*)$/.exec(range.trim());
    if (!m) {
      res.writeHead(416, { "Content-Range": `bytes */${info.size}` }).end();
      return;
    }
    const start = m[1] ? Number(m[1]) : 0;
    const end = m[2] ? Number(m[2]) : info.size - 1;
    if (start >= info.size || end >= info.size || start > end) {
      res.writeHead(416, { "Content-Range": `bytes */${info.size}` }).end();
      return;
    }
    console.log(`206 ${rel}  bytes ${start}-${end}/${info.size}`);
    res.writeHead(206, {
      "Content-Type": type,
      "Content-Length": end - start + 1,
      "Content-Range": `bytes ${start}-${end}/${info.size}`,
      "Accept-Ranges": "bytes",
      "Cache-Control": "no-store",
    });
    if (req.method === "HEAD") { res.end(); return; }
    const stream = createReadStream(abs, { start, end });
    try {
      await (throttle ? pipeline(stream, limiter(throttle), res) : pipeline(stream, res));
    } catch { /* client went away mid-transfer; that is the test */ }
    return;
  }

  console.log(`200 ${rel}  ${info.size} bytes${allowRange ? "" : "  (range disabled)"}`);
  res.writeHead(200, {
    "Content-Type": type,
    "Content-Length": info.size,
    "Accept-Ranges": allowRange ? "bytes" : "none",
    "Cache-Control": "no-store",
  });
  if (req.method === "HEAD") { res.end(); return; }
  const stream = createReadStream(abs);
  try {
    await (throttle ? pipeline(stream, limiter(throttle), res) : pipeline(stream, res));
  } catch { /* client disconnected */ }
});

server.listen(port, () => {
  console.log(`serving ${root}`);
  console.log(`  http://localhost:${port}/`);
  console.log(`  range requests: ${allowRange ? "enabled" : "DISABLED (testing fallback)"}`);
  if (throttle) console.log(`  throttled to ${args.throttle} MB/s`);
  console.log("\nLAN: use this machine's IP instead of localhost when testing from the other PC.");
});
