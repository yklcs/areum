const jsx = (element, props) => {
  if (typeof element === "function") {
    return {
      vtag: element.name,
      ...element(props),
    };
  }

  const { children, ...rest } = props;

  return {
    tag: element,
    children,
    props: rest,
  };
};

const jsxs = jsx;

const Fragment = ({ children }) => ({
  vtag: "Fragment",
  children,
  props: {},
});

export { jsx, jsxs, Fragment };
