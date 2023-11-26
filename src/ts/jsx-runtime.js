const jsx = (element, props) => {
  if (typeof element === "function") {
    return element({ ...props });
  }

  const { children, ...rest } = props;

  return {
    element,
    children,
    props: rest,
  };
};

const jsxs = jsx;

export { jsx, jsxs };
