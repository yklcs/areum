# Areum

_Currently experimental_

Areum is a static site generator.
The core idea is to embed the Deno runtime into a static site generator, allowing any JS (+ JSX, TS, TSX = `/[JT]SX?/`) file to be used to author pages programatically.

## The idea

Templating sucks.
Some sort of templating is needed for a static site generator though, so we turn to JSX.
Using JSX -- a JS extension -- makes a lot of sense since JS is the _lingua franca_ of web development already, and it provides a lot of flexibility and interoperability over text-based templating.
Or even regular JS and template literals go a pretty long way, so Areum uses `/[JT]SX?/`.

Current JSX-based SSGs (Next, Gatsby, Astro, Lume, etc.) are written for Node or maybe Deno.
This comes with the mess of having to manage a runtime, the disaster `node_modules` is, and a lot of general cruft.
Areum solves this by embedding the Deno runtime into a Rust binary.
This allows us to use pages written in `/[JT]SX?/` with just a single executable.

JSX requires some sort of JSX factory (`react/jsx-runtime`, `React.createElement`, etc.) to be turned into something representing a DOM tree.
This is done within the embedded Deno runtime with a custom factory, which builds up an object tree from JSX.
Then the tree is `export default`ed to Rust as a struct and serialized into HTML.

So we can write HTML with `/[JT]SX?/`, but that is only part of the equation.
Pages = content + styles + scripts.
We need to bundle styles and scripts and reintroduce those into the DOM.

For styling a page, a page source file should `export { styles }`, where `styles` is a CSS `string`.
The style string is imported in Rust then injected into the DOM with `<style>`.
This minimal approach allows the usage of different styling techniques based on requirements.
A minimal page could simply use raw HTML, while a more complex page could use imported CSS files or CSS-in-JS transformed to a CSS string, all up to the user to implement.

Likewise, `export { script }` describles the scripting of the page.
The script is injected into the DOM with `<script>`.
Eventually, this is hoped to be replaced with rehydration for better interactivity (see below).

Note that Areum is completely unopinionated on components.
As far as Areum is concerned, components aren't even a thing, just an implementation detail for pages left up to the developer.

### Rehydration?

Using `export { script }` with `script: string` forces us to author scripts as a string which is pretty awful.
This could be solved with rehydration.

A page source file is initially rendered into HTML during build time using the page that has been `export default`ed.
When the page is opened in the browser, `import { script }` is done for the script that has been `export { script }`ed from the page source file.
This allows the build runtime and browser to access the same code.

For example, this is `page.tsx`:

```tsx
import { fn } from "somelib";

const Page = () => <div>hello</div>;

const script = () => {
  /* this should run in the browser */
  console.log(fn());
};

export default Page;
export { script };
```

Which results in the following HTML:

```html
<head>
  <script>
    import { script } from "./page.tsx";
    script();
  </script>
</head>
<body>
  <div>hello</div>
</body>
```

Of course, proper bundling and transpilation would have to be performed for the browser.
