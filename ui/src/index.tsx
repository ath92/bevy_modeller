/* @refresh reload */
import { render } from "solid-js/web";
import css from "./index.css?inline";
import App from "./App.tsx";

const root = document.getElementById("root");

render(
  () => (
    <>
      <App />
      <style>{css}</style>
    </>
  ),
  root!,
);
