/* @refresh reload */
import { render } from "solid-js/web";
import css from "./index.css?inline";
import App from "./App.tsx";
import { RustEvent } from "./types/rust_event.ts";

const root = document.getElementById("root");

window.dispatch_bevy_event = (name: string, detail: RustEvent) => {
  const event = new CustomEvent(name, {
    detail,
  });
  window.dispatchEvent(event);
};

render(
  () => (
    <>
      <App />
      <style>{css}</style>
    </>
  ),
  root!,
);
