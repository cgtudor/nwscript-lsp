import * as vscode from "vscode";

/**
 * Resolves #include directives by scanning the workspace for .nss files.
 * NWScript includes are flat — `#include "foo"` finds `foo.nss` anywhere in the source tree.
 */
export class IncludeResolver {
  private fileIndex = new Map<string, vscode.Uri>(); // stem → URI
  private indexBuilt = false;

  /** Build the file index by scanning the workspace for all .nss files. */
  async buildIndex(): Promise<void> {
    if (this.indexBuilt) return;
    const files = await vscode.workspace.findFiles("**/*.nss", "**/node_modules/**");
    for (const uri of files) {
      const stem = uri.path.split("/").pop()?.replace(/\.nss$/i, "") ?? "";
      if (stem) {
        // Last-write-wins matches NWN override behavior
        this.fileIndex.set(stem.toLowerCase(), uri);
      }
    }
    this.indexBuilt = true;
  }

  /** Invalidate the index (call on workspace file changes). */
  invalidate(): void {
    this.indexBuilt = false;
    this.fileIndex.clear();
  }

  /**
   * Resolve all #include directives for the given source, returning
   * a combined source string with all dependencies prepended.
   */
  async resolveAll(source: string, currentUri?: vscode.Uri): Promise<string> {
    await this.buildIndex();

    const resolved = new Set<string>(); // lowercase stems already included
    const parts: string[] = [];

    // Extract includes from source
    const includes = this.extractIncludes(source);

    // Recursively resolve each include
    for (const inc of includes) {
      await this.resolveRecursive(inc, resolved, parts);
    }

    // Append the main file source (with #include lines stripped for the interpreter)
    parts.push(this.stripIncludes(source));

    return parts.join("\n");
  }

  private async resolveRecursive(
    stem: string,
    resolved: Set<string>,
    parts: string[]
  ): Promise<void> {
    const key = stem.toLowerCase();
    if (resolved.has(key)) return;
    resolved.add(key);

    const uri = this.fileIndex.get(key);
    if (!uri) {
      // File not found — skip silently (builtins handle nw_inc_nui etc.)
      return;
    }

    let content: string;
    try {
      const bytes = await vscode.workspace.fs.readFile(uri);
      content = Buffer.from(bytes).toString("utf-8");
    } catch {
      return;
    }

    // Recursively resolve this file's includes first
    const subIncludes = this.extractIncludes(content);
    for (const sub of subIncludes) {
      await this.resolveRecursive(sub, resolved, parts);
    }

    // Add this file's content (without #include lines)
    parts.push(`// === ${stem}.nss ===`);
    parts.push(this.stripIncludes(content));
  }

  private extractIncludes(source: string): string[] {
    const includes: string[] = [];
    const regex = /^\s*#include\s+["<]([^">]+)[">]/gm;
    let match;
    while ((match = regex.exec(source)) !== null) {
      includes.push(match[1]);
    }
    return includes;
  }

  private stripIncludes(source: string): string {
    return source.replace(/^\s*#include\s+["<][^">]+[">]\s*$/gm, "// (include resolved)");
  }
}
