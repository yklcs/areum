import Component, { styles } from "./_component.tsx";
import { format } from "https://deno.land/std@0.208.0/datetime/mod.ts";

const Page = () => (
  <main>
    <h1>Hello world!</h1>
    <Component>
      <span>Today is:</span>
      <time>{format(new Date(), "yyyy-MM-dd")}</time>
    </Component>
  </main>
);

const scripts = () => {
  console.log(format(new Date(), "yyyy-MM-dd"));
};

export default Page;
export { scripts, styles };
