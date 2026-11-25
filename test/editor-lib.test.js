import assert from "node:assert/strict";
import test from "node:test";
import {
  assertWritableChangePath,
  extractPagePaths,
  joinDeckPath,
  normalizeRelativePath,
  titleFromManifest,
} from "../editor/lib.js";

const yaml = `---
version: v2
title: "测试文稿"
pages:
  - pages/01.page
  - 'pages/02.page' # comment
theme:
  colors: {}
`;

test("parses PPTD manifests and keeps file access inside the project", () => {
  assert.deepEqual(extractPagePaths(yaml), ["pages/01.page", "pages/02.page"]);
  assert.deepEqual(extractPagePaths('{"pages":["a.page","folder/b.page"]}'), ["a.page", "folder/b.page"]);
  assert.equal(titleFromManifest(yaml, "fallback"), "测试文稿");
  assert.equal(joinDeckPath("deck", "pages/01.page"), "deck/pages/01.page");
  assert.equal(joinDeckPath("deck", "deck/pages/01.page"), "deck/pages/01.page");
  assert.equal(assertWritableChangePath("deck", "pages/01.page"), "deck/pages/01.page");
  assert.throws(() => normalizeRelativePath("../secret"), /不允许越过/);
  assert.throws(() => normalizeRelativePath("..%5csecret"), /不允许越过/);
  assert.throws(() => normalizeRelativePath("%2e%2e/secret"), /不允许越过/);
  assert.throws(() => normalizeRelativePath("/absolute/path"), /绝对路径/);
  assert.throws(() => normalizeRelativePath("file:///etc/passwd"), /绝对路径/);
  assert.throws(() => normalizeRelativePath("https://example.com/a.page"), /绝对路径/);
  assert.throws(() => normalizeRelativePath("C:/secret"), /绝对路径/);
  assert.throws(() => normalizeRelativePath("C:%5csecret"), /绝对路径/);
  assert.throws(() => normalizeRelativePath("pages/01\0.page"), /绝对路径/);
  assert.throws(() => normalizeRelativePath("pages/01%00.page"), /绝对路径/);
  assert.throws(() => assertWritableChangePath("deck", "media/image.png"), /仅允许编辑/);
});

test("extracts inline YAML pages arrays", () => {
  assert.deepEqual(extractPagePaths("pages: [pages/01.page]\n"), ["pages/01.page"]);
  assert.deepEqual(
    extractPagePaths("title: Demo\npages: [\"pages/01.page\", 'pages/02.page'] # inline\n"),
    ["pages/01.page", "pages/02.page"],
  );
  assert.throws(() => extractPagePaths("pages: []\n"), /为空/);
});
