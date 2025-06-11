function App() {
  return (
    <div class="left">
      <h2>Spawn</h2>
      <button onClick={() => window.wasmBindings.spawn_sphere_at_origin()}>
        New sphere at origin
      </button>
    </div>
  );
}

export default App;
