// Single source of truth for the platform matrix, shared by the launcher that
// ships to users and by the script that builds the platform packages. The
// targets mirror dist-workspace.toml; adding one here without adding it there
// produces a package that never gets a binary.

export const platforms = [
  {
    target: "aarch64-apple-darwin",
    suffix: "darwin-arm64",
    os: "darwin",
    cpu: "arm64",
    archive: "tar.gz",
  },
  {
    target: "x86_64-apple-darwin",
    suffix: "darwin-x64",
    os: "darwin",
    cpu: "x64",
    archive: "tar.gz",
  },
  {
    target: "aarch64-unknown-linux-gnu",
    suffix: "linux-arm64-gnu",
    os: "linux",
    cpu: "arm64",
    libc: "glibc",
    archive: "tar.gz",
  },
  {
    target: "x86_64-unknown-linux-gnu",
    suffix: "linux-x64-gnu",
    os: "linux",
    cpu: "x64",
    libc: "glibc",
    archive: "tar.gz",
  },
  {
    target: "aarch64-unknown-linux-musl",
    suffix: "linux-arm64-musl",
    os: "linux",
    cpu: "arm64",
    libc: "musl",
    archive: "tar.gz",
  },
  {
    target: "x86_64-unknown-linux-musl",
    suffix: "linux-x64-musl",
    os: "linux",
    cpu: "x64",
    libc: "musl",
    archive: "tar.gz",
  },
  {
    target: "x86_64-pc-windows-msvc",
    suffix: "win32-x64-msvc",
    os: "win32",
    cpu: "x64",
    archive: "zip",
  },
];

export const packageName = (platform) => `@deckflow/deckprobe-${platform.suffix}`;

export const binaryName = (platform) =>
  platform.os === "win32" ? "deckprobe.exe" : "deckprobe";

/**
 * glibc and musl builds are not interchangeable, and npm's "libc" field is only
 * honoured by newer npm and pnpm. Detect at runtime as well: Node reports a
 * glibc runtime version only when it is actually linked against glibc.
 */
export function detectLibc() {
  if (process.platform !== "linux") return undefined;
  const report =
    typeof process.report?.getReport === "function" ? process.report.getReport() : undefined;
  return report?.header?.glibcVersionRuntime ? "glibc" : "musl";
}

export function currentPlatform() {
  const libc = detectLibc();
  return platforms.find(
    (platform) =>
      platform.os === process.platform &&
      platform.cpu === process.arch &&
      platform.libc === libc,
  );
}
