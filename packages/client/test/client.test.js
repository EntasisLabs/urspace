import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

test("package exports the embed API", async () => {
  const pkg = JSON.parse(await readFile(join(root, "package.json"), "utf8"));
  assert.equal(pkg.name, "@urspace/client");
  const source = await readFile(join(root, "src/index.js"), "utf8");
  assert.match(source, /export async function connect/);
  assert.match(source, /export async function connectMinted/);
  assert.match(source, /export function generateSessionKey/);
  const types = await readFile(join(root, "src/index.d.ts"), "utf8");
  assert.match(types, /export function connectMinted/);
});
