import { jsx, render, type JSX } from "/areum/jsx-runtime";

const load = async (url: string, props: JSX.PageProps) => {
  const mod = (await import(url)).default;

  let fn: JSX.FunctionalElement;
  if (typeof mod === "function") {
    fn = mod;
  } else {
    fn = mod[props.path];
  }

  const page = jsx(fn, props);
  return render(page);
};

export default load;
