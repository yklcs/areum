const randString: (n: number) => string =
  Deno.core.ops.randString;

const runScript = (node: Node) => {
  if (node.kind === "virtual" && node.script) {
    node.script();
  }

  if (Array.isArray(node.children)) {
    for (const child of node.children) {
      runScript(child);
    }
  } else if (node.children && typeof node.children !== "string") {
    runScript(node.children);
  }
};

type Children = Node | string | Children[];

interface IntrinsicNode {
  kind: "intrinsic";
  props: JSX.Props;
  children: Children;
  scope: string;

  tag: string;
}

interface VirtualNode {
  kind: "virtual";
  props: JSX.Props;
  children: Children;
  scope: string;

  style?: string;
  script?: () => void;
}

type Node = IntrinsicNode | VirtualNode;

const applyScopeChildren = (children: JSX.Children, scope: string) => {
  if (typeof children === "string") {
  } else if (Array.isArray(children)) {
    children.forEach((child) => applyScopeChildren(child, scope));
  } else if (children) {
    applyScope(children, scope);
  }
};

const applyScope = (element: JSX.Element, scope: string) => {
  if (element.props && !element.props.__scope) {
    element.props.__scope = scope;
  } else if (!element.props) {
    element.props = { __scope: scope };
  }

  applyScopeChildren(element.children, scope);
};

const renderChildren = (children: JSX.Children): Children => {
  let rendered: Children;

  if (typeof children === "string") {
    rendered = children;
  } else if (Array.isArray(children)) {
    rendered = children.map((child) => renderChildren(child)).filter((x) => x);
  } else if (children) {
    rendered = render(children);
  }

  return rendered;
};

const render = (element: JSX.Element): Node | undefined => {
  let node_: Node;

  if (!element || (Array.isArray(element) && element.length === 0)) {
    return undefined;
  }

  if (typeof element.element === "function") {
    let node = {} as VirtualNode;
    node.kind = "virtual";

    const newScope = randString(8);

    if (element.element !== Fragment) {
      applyScope(element, newScope);
      element.props.__scope = newScope;
      const inner = element.element({
        ...element.props,
        children: element.children,
      });
      applyScope(inner, newScope);
      node.children = render(inner);
    } else {
      node.children = renderChildren(element.children);
    }

    if (typeof element.element.style === "function") {
      node.style = element.element.style(element.props);
    } else {
      node.style = element.element.style;
    }
    node.script = element.element.script;

    node_ = node;
  } else {
    let node = {} as IntrinsicNode;
    node.kind = "intrinsic";

    node.tag = element.element;

    node.children = renderChildren(element.children);

    node_ = node;
  }

  node_.props = { __scope: "", ...element.props };
  node_.scope = node_.props.__scope;

  return node_;
};

const jsx = (element: JSX.ElementType, props: JSX.Props) => {
  let { children, ...rest } = props;

  const node: JSX.Element = {
    element,
    children,
    props: rest,
  };

  return node;
};

const jsxs = jsx;

const Fragment = ({ children }: JSX.Props) => children;

namespace JSX {
  // TypeScript

  export interface IntrinsicElements {
    [el: string]: unknown;
  }

  export interface IntrinsicAttributes {
    cascade?: boolean;
  }

  export interface ElementChildrenAttribute {
    children: "children";
  }

  export type ElementType =
    | Extract<keyof IntrinsicElements, string>
    | FunctionalElement;

  export interface Element {
    element: ElementType;
    props: Props;
    children?: Children;
  }

  // Custom

  export type Children = string | Element | Children[];

  export interface FunctionalElement {
    (props: Props): Element;
    style?: string | ((props: Props) => string);
    script?: () => void;
  }

  export interface PageProps {
    path: string;
    generator: string;
  }

  export interface Props {
    children?: Children;
    [key: string]: any;
  }
}

export { jsx, jsxs, Fragment, runScript, render, type JSX };
