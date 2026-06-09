Build and publish the VS Code extension for both Windows and Linux.

## Steps

1. Run `cargo test` — abort if any tests fail.
2. Build the Windows release: `cargo build --release -p nwscript-lsp`
3. Build the Linux release: use `cargo zigbuild --release -p nwscript-lsp --target x86_64-unknown-linux-gnu` (requires zig on PATH — a wrapper at `C:\Users\User\.local\bin\zig.cmd` delegates to the pip-installed ziglang package). Alternatively use WSL: `wsl bash -c 'cd /mnt/d/tdn/workspace/nwscript-lsp && cargo build --release -p nwscript-lsp'`
4. Copy Windows binaries into `editors/vscode/bin/`:
   - `target/release/nwscript-lsp.exe`
   - `bin/win64/nwn_script_comp.exe` (bundled compiler)
5. Package Windows VSIX: `cd editors/vscode && npm run package:win`
6. Swap Linux binaries into `editors/vscode/bin/` (remove .exe files first):
   - `target/x86_64-unknown-linux-gnu/release/nwscript-lsp` (or from WSL build)
   - Linux `nwn_script_comp` from `D:/tdn/workspace/nwscript-ee-language-server/server/resources/compiler/linux/nwn_script_comp`
7. Package Linux VSIX: `npm run package:linux`
8. Restore Windows binaries in `editors/vscode/bin/` (for local development)
9. Publish both: `npx @vscode/vsce publish --packagePath nwscript-lsp-win32-x64-VERSION.vsix nwscript-lsp-linux-x64-VERSION.vsix --allow-missing-repository`

Read the version from `editors/vscode/package.json` to construct the VSIX filenames.

Before publishing, confirm with the user that the version number and changelog are correct.
