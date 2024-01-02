import { jsx, render, type JSX } from "/areum/jsx-runtime";

const load = async (url: string, props: JSX.PageProps) => {
  const mod = await import(url);
  const fn: JSX.FunctionalElement = mod.default;

  const page = jsx(fn, props);

  return render(page);
};

export default load;
