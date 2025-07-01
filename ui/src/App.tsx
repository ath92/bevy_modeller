import { createSignal, createEffect, onCleanup } from "solid-js";
import { Mode } from "./types/modes";

function App() {
  const [mode, setMode] = createSignal<Mode>("Translate");
  createEffect(() => {
    const listener = (event: CustomEvent<Mode>) => {
      setMode(event.detail);
    };
    window.addEventListener("modeChanged", listener);
    onCleanup(() => {
      window.removeEventListener("modeChanged", listener);
    });
  });
  return (
    <div class="left">
      <h2>Spawn</h2>
      <button onClick={() => window.wasmBindings.spawn_sphere_at_origin()}>
        New sphere at origin
      </button>

      <button
        classList={{
          active: mode() === "Translate",
        }}
        onClick={() => window.wasmBindings.set_mode("Translate")}
      >
        Translate
      </button>

      <button
        classList={{
          active: mode() === "Brush",
        }}
        onClick={() => window.wasmBindings.set_mode("Brush")}
      >
        Brush
      </button>
    </div>
  );
}

export default App;
