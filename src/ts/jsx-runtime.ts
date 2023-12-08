interface Props {
  children?: Node | Node[];
  [key: string]: any;
}

interface Node {
  vtag?: string;
  tag?: string;
  style?: string;
  children?: Node | Node[];
  props: Props;
}

const jsx = (element: JSX.ElementType, props: Props): Node => {
  if (typeof element === "function") {
    return {
      vtag: element.name,
      style:
        typeof element.style === "function"
          ? element.style(props)
          : element.style,
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

const Fragment = ({ children }: { children?: Node | Node[] }) => ({
  vtag: "Fragment",
  children,
  props: {},
});

export namespace JSX {
  export interface IntrinsicElements {
    [el: string]: unknown;
  }

  export interface ElementChildrenAttribute {
    children: "children";
  }

  export type ElementType =
    | string
    | {
        (props: Props): Node;
        style?: string | ((props: Props) => string);
      };

  export type Element = Node;
}

export { jsx, jsxs, Fragment };
