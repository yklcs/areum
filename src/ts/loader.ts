const load = async (url: string) => {
  const mod = await import(url);
  const fn = mod.default;
  const page = fn();
  page.style = fn.style;
  return page;
};

export default load;
