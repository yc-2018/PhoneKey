import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const fallbackCargo = join(homedir(), ".cargo", "bin", "cargo.exe");
const cargo = process.env.CARGO
  ?? (existsSync(fallbackCargo) ? fallbackCargo : "cargo");

const build = spawnSync(
  cargo,
  ["build", "-p", "unlock_provider", "--release"],
  { stdio: "inherit" },
);
if (build.error) {
  console.error(build.error.message);
  process.exit(1);
}
if (build.status !== 0) {
  process.exit(build.status ?? 1);
}

copyFileSync("target/release/unlock_provider.dll", "installer/UnlockProvider.dll");
console.log("已更新 installer/UnlockProvider.dll");
