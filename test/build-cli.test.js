import assert from "node:assert/strict";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const cli = join(projectRoot, "bin", "open-pptd-skill.js");

function createStubBinary(dir, exitCode = 42, name = "stub-pptd") {
  const isWin = process.platform === "win32";
  const body = `import { writeFileSync } from "node:fs";
const log = process.env.STUB_LOG;
if (log) {
  writeFileSync(log, JSON.stringify(process.argv.slice(2)), "utf8");
}
process.exit(${exitCode});
`;

  if (isWin) {
    const stubJs = join(dir, `${name}.mjs`);
    writeFileSync(stubJs, body, "utf8");
    const stubCmd = join(dir, `${name}.cmd`);
    writeFileSync(stubCmd, `@echo off\r\nnode "${stubJs}" %*\r\n`, "utf8");
    return stubCmd;
  }

  const stubBin = join(dir, name);
  writeFileSync(stubBin, `#!/usr/bin/env node\n${body}`, "utf8");
  chmodSync(stubBin, 0o755);
  return stubBin;
}

test("build forwards to native binary specified by PPTD_BIN with unchanged arguments and exit code", () => {
  const tempDir = mkdtempSync(join(tmpdir(), "pptd-stub-test-"));
  const logFile = join(tempDir, "args.json");
  const stubBinary = createStubBinary(tempDir, 42);

  try {
    const result = spawnSync(
      process.execPath,
      [cli, "build", "deck.pptd", "-o", "out.pptx", "--force"],
      {
        env: {
          ...process.env,
          PPTD_BIN: stubBinary,
          STUB_LOG: logFile,
        },
        encoding: "utf8",
      },
    );

    assert.equal(result.status, 42, `stderr: ${result.stderr}`);
    const recordedArgs = JSON.parse(readFileSync(logFile, "utf8"));
    assert.deepEqual(recordedArgs, ["build", "deck.pptd", "-o", "out.pptx", "--force"]);
  } finally {
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("build forwards exit code 0 on success", () => {
  const tempDir = mkdtempSync(join(tmpdir(), "pptd-stub-test-"));
  const logFile = join(tempDir, "args.json");
  const stubBinary = createStubBinary(tempDir, 0);

  try {
    const result = spawnSync(process.execPath, [cli, "build", "deck.pptd"], {
      env: {
        ...process.env,
        PPTD_BIN: stubBinary,
        STUB_LOG: logFile,
      },
      encoding: "utf8",
    });

    assert.equal(result.status, 0, `stderr: ${result.stderr}`);
    const recordedArgs = JSON.parse(readFileSync(logFile, "utf8"));
    assert.deepEqual(recordedArgs, ["build", "deck.pptd"]);
  } finally {
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("convert, check, and dump forward directly to native pptd binary", () => {
  const tempDir = mkdtempSync(join(tmpdir(), "pptd-forward-test-"));
  const logFile = join(tempDir, "args.json");
  const stubBinary = createStubBinary(tempDir, 7);

  try {
    const cases = [
      ["convert", "in.pptx", "out_dir", "--json"],
      ["check", "deck.pptd"],
      ["dump", "deck.pptd"],
    ];
    for (const args of cases) {
      const result = spawnSync(process.execPath, [cli, ...args], {
        env: {
          ...process.env,
          PPTD_BIN: stubBinary,
          STUB_LOG: logFile,
        },
        encoding: "utf8",
      });
      assert.equal(result.status, 7, `${args[0]} stderr: ${result.stderr}`);
      assert.deepEqual(JSON.parse(readFileSync(logFile, "utf8")), args);
    }
  } finally {
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("discovers native pptd binary in ~/.local/bin/pptd", { skip: process.platform === "win32" }, () => {
  const tempDir = mkdtempSync(join(tmpdir(), "pptd-localbin-test-"));
  const localBinDir = join(tempDir, ".local", "bin");
  mkdirSync(localBinDir, { recursive: true });
  const logFile = join(tempDir, "args.json");
  createStubBinary(localBinDir, 0, "pptd");

  try {
    const result = spawnSync(process.execPath, [cli, "check", "deck.pptd"], {
      env: {
        ...process.env,
        PPTD_BIN: "",
        HOME: tempDir,
        USERPROFILE: tempDir,
        PATH: dirname(process.execPath),
        STUB_LOG: logFile,
      },
      encoding: "utf8",
    });

    assert.equal(result.status, 0, `stderr: ${result.stderr}`);
    assert.deepEqual(JSON.parse(readFileSync(logFile, "utf8")), ["check", "deck.pptd"]);
  } finally {
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("discovers native pptd binary via PATH", () => {
  const tempDir = mkdtempSync(join(tmpdir(), "pptd-path-test-"));
  const binDir = join(tempDir, "bin");
  mkdirSync(binDir, { recursive: true });
  const logFile = join(tempDir, "args.json");
  createStubBinary(binDir, 0, "pptd");
  const delimiter = process.platform === "win32" ? ";" : ":";

  try {
    const result = spawnSync(process.execPath, [cli, "dump", "deck.pptd"], {
      env: {
        ...process.env,
        PPTD_BIN: "",
        HOME: tempDir,
        USERPROFILE: tempDir,
        PATH: `${binDir}${delimiter}${dirname(process.execPath)}`,
        STUB_LOG: logFile,
      },
      encoding: "utf8",
    });

    assert.equal(result.status, 0, `stderr: ${result.stderr}`);
    assert.deepEqual(JSON.parse(readFileSync(logFile, "utf8")), ["dump", "deck.pptd"]);
  } finally {
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("missing native binary gives actionable install instructions and non-zero exit code (custom PPTD_BIN)", () => {
  const tempDir = mkdtempSync(join(tmpdir(), "pptd-missing-test-"));
  const missingBinary = join(tempDir, "missing-pptd-binary");

  try {
    const result = spawnSync(process.execPath, [cli, "build", "deck.pptd", "-o", "out.pptx"], {
      env: {
        ...process.env,
        PPTD_BIN: missingBinary,
      },
      encoding: "utf8",
    });

    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /native 'pptd' binary not found/);
    assert.match(result.stderr, /skills\/open-pptd\/scripts\/install_binary\.py/);
    assert.doesNotMatch(result.stderr, /spawnSync is not defined/);
    assert.doesNotMatch(result.stderr, /unknown argument/);
  } finally {
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("missing native binary gives actionable install instructions and non-zero exit code (no PATH or local bin)", () => {
  const tempDir = mkdtempSync(join(tmpdir(), "pptd-empty-env-test-"));

  try {
    const result = spawnSync(process.execPath, [cli, "build", "deck.pptd"], {
      env: {
        ...process.env,
        PPTD_BIN: "",
        HOME: tempDir,
        USERPROFILE: tempDir,
        PATH: "",
      },
      encoding: "utf8",
    });

    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /native 'pptd' binary not found/);
    assert.match(result.stderr, /skills\/open-pptd\/scripts\/install_binary\.py/);
    assert.doesNotMatch(result.stderr, /spawnSync is not defined/);
    assert.doesNotMatch(result.stderr, /unknown argument/);
  } finally {
    rmSync(tempDir, { recursive: true, force: true });
  }
});
