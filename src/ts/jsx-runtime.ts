interface Props {
  children?: Node | Node[];
  [key: string]: any;
}

interface Node {
  vtag?: string;
  tag?: string;
  style?: string;
  script?: () => void;
  children?: Node | Node[];
  props: Props;
}

const runScript = (node: Node) => {
  if (node.script) {
    node.script();
  }
  if (Array.isArray(node.children)) {
    for (const child of node.children) {
      runScript(child);
    }
  } else if (node.children) {
    runScript(node.children);
  }
};

const jsx = (element: JSX.ElementType, props: Props): Node => {
  let node;

  if (typeof element === "function") {
    node = {
      vtag: element.name,
      style:
        typeof element.style === "function"
          ? element.style(props)
          : element.style,
      script: element.script,
      ...element(props),
    };
  } else {
    const { children, ...rest } = props;
    node = {
      tag: element,
      children,
      props: rest,
    };
  }

  console.log(node);
  return node;
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
        script?: () => void;
      };

  export type Element = Node;
}

export { jsx, jsxs, Fragment, runScript };
