/**
 * TypeScript definitions for WASM bindings exposed by the Rust backend.
 * These functions are set on window.wasmBindings by Trunk during the build process.
 */

export interface WasmBindings {
  /**
   * Spawns a sphere at the specified position with the given color.
   * @param x - X coordinate position
   * @param y - Y coordinate position  
   * @param z - Z coordinate position
   * @param r - Red color component (0.0 to 1.0)
   * @param g - Green color component (0.0 to 1.0)
   * @param b - Blue color component (0.0 to 1.0)
   * @returns A string describing the spawned sphere
   */
  spawn_sphere(x: number, y: number, z: number, r: number, g: number, b: number): string;

  /**
   * Convenience function to spawn a sphere at the origin (0, 0, 0) with a default blue color.
   * @returns A string describing the spawned sphere
   */
  spawn_sphere_at_origin(): string;
}

declare global {
  interface Window {
    /**
     * WASM bindings exposed by the Rust backend.
     * These functions are automatically set by Trunk during the build process.
     */
    wasmBindings: WasmBindings;
  }
}

// This export is needed to make this file a module
export {};