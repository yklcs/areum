interface Props {
  children?: Node | Node[];
  [key: string]: unknown;
}

type Element = ((props: Props) => Node) | string;

interface Node {
  vtag?: string;
  tag?: string;
  children?: Node | Node[];
  props: Props;
}

const jsx = (element: Element, props: Props): Node => {
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

const Fragment = ({ children }: { children: Node | Node[] }) => ({
  vtag: "Fragment",
  children,
  props: {},
});

export { jsx, jsxs, Fragment };
