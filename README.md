# Areum

_Currently experimental_

Areum is a static site generator.
The core idea is to embed the Deno runtime into a static site generator, allowing any JSX/TSX file to be used to author pages programatically.

- Single Rust binary with embedded Deno runtime
- JSX/TSX based pages
- Property based styling and scripting
- CSS/JS/asset processing pipeline

```tsx
import Layout from "./_Layout.tsx";

const date = new Date();

const Page = () => (
  <Layout>
    <h1 class="red">Hello world!</h1>
    <p>Build date: ${date}</p>
  </Layout>
);

Page.style = `
	.red {
		color: red;
	}
`;

Page.script = () => {
  console.log("This function runs in the browser.");
  console.log("Current (browser) date:", date);
};

export default Page;
```
