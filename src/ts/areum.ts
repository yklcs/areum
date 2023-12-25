import * as deno_path from "https://deno.land/std@0.210.0/path/mod.ts";

globalThis.Deno.core = Deno[Deno.internal].core;

const Areum = {
  root: (): string => Deno.core.ops.op_root(),
  path: (importMeta: ImportMeta): string => {
    const page = deno_path.fromFileUrl(importMeta.url);
    const relative = deno_path.relative(Areum.root(), page);
    const relativePath = deno_path.parse(relative);

    const matches = ["index.tsx", "index.jsx", "index.mdx", "index.md"];
    for (const match of matches) {
      if (relativePath.base == match) {
        return deno_path.join("/", relativePath.dir);
      }
    }
    return deno_path.join("/", relativePath.dir, relativePath.name);
  },
};

globalThis.Areum = Areum;

declare global {
  interface AreumGlobal {
    root: () => string;
    path: (ImportMeta) => string;
  }

  var Areum: AreumGlobal;
}

export {};
