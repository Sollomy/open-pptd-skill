import assert from "node:assert/strict";
import { once } from "node:events";
import test from "node:test";
import { createEditorServer } from "../lib/editor-server.js";

async function withServer(callback) {
  const server = createEditorServer();
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();

  try {
    await callback(`http://127.0.0.1:${address.port}`);
  } finally {
    server.close();
    await once(server, "close");
  }
}

test("serves the PPTD editor and its JavaScript modules", async () => {
  await withServer(async (url) => {
    const index = await fetch(`${url}/`);
    assert.equal(index.status, 200);
    assert.match(index.headers.get("content-type"), /^text\/html/);
    assert.match(await index.text(), /打开 PPTD 文件夹/);

    const app = await fetch(`${url}/app.js`);
    assert.equal(app.status, 200);
    assert.match(app.headers.get("content-type"), /^text\/javascript/);
  });
});

test("returns 404 for missing files", async () => {
  await withServer(async (url) => {
    const response = await fetch(`${url}/missing.js`);
    assert.equal(response.status, 404);
  });
});

test("rejects path traversal, malformed URIs, and non-GET/HEAD methods", async () => {
  await withServer(async (url) => {
    const traversal = await fetch(`${url}/..%2fpackage.json`);
    assert.equal(traversal.status, 403);

    const malformed = await fetch(`${url}/%ZZ`);
    assert.equal(malformed.status, 400);

    const post = await fetch(`${url}/`, { method: "POST" });
    assert.equal(post.status, 405);
  });
});

test("supports HEAD requests without a response body", async () => {
  await withServer(async (url) => {
    const response = await fetch(`${url}/styles.css`, { method: "HEAD" });
    assert.equal(response.status, 200);
    assert.equal(await response.text(), "");
  });
});
