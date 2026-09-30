// The How To Use fingerprint: what the release publishes in how-to-use.json.
// The app compares that published string with the one it installed; it
// never recomputes it. src-tauri/src/guide.rs (`fingerprint`) implements the
// same rule for its tests, and scripts/guide-fingerprint.test.mjs and
// tests/suite/guide.rs pin the same values for the same fixtures.
//
// The rule: every regular file under the site root (symlinks are skipped);
// its path relative to the root with "/" separators; sorted by the path's
// UTF-8 bytes; for each, the text "<path>\n<sha256 hex of its bytes>\n";
// the fingerprint is the SHA-256 hex of the concatenation.
//
// Usage: node scripts/guide-fingerprint.mjs <site-root>   (prints the hex)
import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const sha256 = (data) => createHash("sha256").update(data).digest("hex");

function listFiles(root, rel = "") {
  const out = [];
  for (const entry of readdirSync(join(root, rel), { withFileTypes: true })) {
    const path = rel ? `${rel}/${entry.name}` : entry.name;
    if (entry.isDirectory()) out.push(...listFiles(root, path));
    else if (entry.isFile()) out.push(path);
  }
  return out;
}

export function fingerprint(dir) {
  const paths = listFiles(dir).sort((a, b) =>
    Buffer.compare(Buffer.from(a, "utf8"), Buffer.from(b, "utf8")),
  );
  const all = createHash("sha256");
  for (const path of paths) {
    const digest = sha256(readFileSync(join(dir, ...path.split("/"))));
    all.update(`${path}\n${digest}\n`, "utf8");
  }
  return all.digest("hex");
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  if (!process.argv[2]) {
    console.error("usage: node scripts/guide-fingerprint.mjs <site-root>");
    process.exit(2);
  }
  console.log(fingerprint(process.argv[2]));
}
