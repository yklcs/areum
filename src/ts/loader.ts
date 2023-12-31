import { type JSX } from "areum/jsx-runtime";

const load = async (url: string, props: JSX.PageProps) => {
  const mod = await import(url);
  const fn = mod.default;
  const page = fn(props);
  if (page.style !== undefined && fn.style !== undefined) {
    page.style = page.style + fn.style;
  } else if (fn.style !== undefined) {
    page.style = fn.style;
  }
  return page;
};

export default load;
