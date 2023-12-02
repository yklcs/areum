const jsx = (element, props) => {
  if (typeof element === "function") {
    return {
      type: "virtual",
      data: { vtag: element.name, inner: element(props) },
    };
  }

  const { children, ...rest } = props;

  return {
    type: "html",
    data: {
      element,
      children,
      props: rest,
    },
  };
};

const jsxs = jsx;

export { jsx, jsxs };
