import Component, { styles } from "./_component.tsx";
import { format } from "https://deno.land/std@0.208.0/datetime/mod.ts";

const HelloWorldNest = () => <HelloWorld />;
const HelloWorldNestNest = () => <HelloWorldNest />;
const HelloWorld = () => <h1>Hello world!</h1>;

const Page = () => (
  <main>
    <HelloWorldNestNest />
    <Component color="red">
      <>
      <span>Today is:</span>
      <time>{format(new Date(), "yyyy-MM-dd")}</time>
      </>
    </Component>
    <style>
      {`span {
        color: blue;
      }`}
    </style>
  </main>
);

const scripts = () => {
  console.log(format(new Date(), "yyyy-MM-dd"));
};

export default Page;
export { scripts, styles };
