# Areum

_Currently experimental_

Areum is a single executable static site generator focusing on JSX and MDX execution.

- Single Rust binary with embedded Deno runtime
- JSX/TSX/MDX/MD based pages
- Server mode with live reloading
- Property based styling and scripting
- CSS/JS/asset processing pipeline

```tsx
import Layout from "./_Layout.tsx";

const Counter = () => {
  const id = "counter";
  let state = 0;

  const Element = (
    <div id={id}>
      <span>`${state}`</span>
      <button>increment</button>
    </div>
  );

  Element.script = () => {
    const count = document.querySelector(`${id} > span`);
    const button = document.querySelector(`${id} > button`);
    button.addEventListener("click", () => {
      count.innerHTML = `${++state}`;
    });
  };

  return Element;
};

const Page = () => (
  <Layout>
    <h1 class="red">Hello world!</h1>
    <Counter />
    <p>Build date: ${new Date()}</p>
  </Layout>
);

Page.style = `
  .red {
    color: red;
  }
`;

export default Page;
```

## Usage

```shell
# Build site
$ areum build src/

# Start server
$ areum serve src/
```
