import { jsx, render, type JSX } from "/areum/jsx-runtime";

const load = async (url: string, props: JSX.PageProps) => {
  const fn = (await import(url)).default;
  const page = jsx(fn, props);
  return render(page);
};

const loadGenerator = async (url: string, props: JSX.PageProps) => {
  const mods = (await import(url)).default;
  const root = props.path;

  let entries = Object.entries(mods).map(([relpath, fn]) => {
    const path = Deno.core.ops.join_path(root, relpath);
    const page_props = { ...props, path };
    const page = jsx(fn, page_props);

    return [path, render(page)];
  });

  return new Map(entries);
};

export { load, loadGenerator };
