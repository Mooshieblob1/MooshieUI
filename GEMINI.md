### Project Overview

This project, MooshieUI, is a desktop application that provides a user-friendly interface for ComfyUI, a powerful node-based backend for image generation. The application is built using Svelte 5 for the frontend and Tauri (with Rust) for the desktop framework and backend logic.

The goal of MooshieUI is to abstract away the complexity of ComfyUI's graph-based workflow, offering a more intuitive experience for users. It supports various generation modes like Text-to-Image, Image-to-Image, and Inpainting, along with a rich set of controls for models, samplers, dimensions, and upscaling.

**Key Technologies:**
- **Frontend:** Svelte 5, TypeScript, Tailwind CSS
- **Backend & Desktop:** Tauri v2 (Rust)
- **State Management:** Svelte 5 Runes (`$state`, `$derived`)
- **API Communication:** The frontend communicates with the Rust backend via Tauri's `invoke` mechanism. The Rust backend then constructs and sends workflow requests to the ComfyUI Python backend via its REST and WebSocket APIs.

**Architecture:**
- `src/`: Contains the Svelte 5 frontend application.
    - `lib/components/`: Reusable UI components.
    - `lib/stores/`: Svelte 5 rune-based stores for global state management.
    - `lib/utils/api.ts`: Bridge to the Tauri backend commands.
- `src-tauri/`: Contains the Rust backend.
    - `src/commands/`: Tauri command handlers exposed to the frontend.
    - `src/comfyui/`: Logic for interacting with the ComfyUI backend (client, process management, WebSocket).
    - `src/templates/`: Builders for generating ComfyUI workflow JSON.
- `comfyui-nodes/`: Custom Python nodes for ComfyUI, such as for tiled diffusion.

The application works by taking user input from the Svelte UI, sending it to the Rust backend, which then dynamically builds a ComfyUI workflow JSON and submits it to the ComfyUI server. Progress is streamed back to the UI in real-time using WebSockets.

### Building and Running

**Prerequisites:**
- Node.js 18+
- Rust (latest stable)
- Tauri v2 prerequisites (see [Tauri docs](https://v2.tauri.app/start/prerequisites/))

**Development:**
To run the application in development mode with hot-reloading:
1. Install frontend dependencies: `npm install`
2. Run the development server: `npm run tauri dev`

The application has a one-time setup wizard that will automatically download and configure Python, ComfyUI, and all required dependencies on first launch.

**Production Build:**
To build the application for production:
1. Install frontend dependencies: `npm install`
2. Run the build command: `npm run tauri build`

This will create a standalone executable/installer in `src-tauri/target/release/`.

**Key Scripts from `package.json`:**
- `dev`: `vite` (Runs the Svelte frontend dev server)
- `build`: `vite build` (Builds the Svelte frontend)
- `tauri`: `tauri` (Main command for interacting with the Tauri CLI)
- `tauri dev`: Starts the development environment.
- `tauri build`: Creates a production build.

### Development Conventions

- **State Management:** Global application state is managed using Svelte 5 runes (`$state`, `$derived`) in the `src/lib/stores/` directory. This provides a reactive and easy-to-follow state management pattern.
- **Backend Communication:** All interactions with the backend are handled through Tauri commands defined in `src-tauri/src/commands/` and called from the frontend using the `@tauri-apps/api` package. The `src/lib/utils/api.ts` file acts as a bridge, wrapping these `invoke` calls.
- **Styling:** The UI is styled using Tailwind CSS 4.
- **Typing:** The project uses TypeScript. Type definitions for complex objects are located in `src/lib/types/`.
- **Component Structure:** The UI is broken down into modular components located in `src/lib/components/`, organized by feature (e.g., `generation`, `canvas`, `progress`).
- **Backend Structure:** The Rust backend is organized by functionality: `commands` for the API layer, `comfyui` for business logic related to the ComfyUI integration, and `templates` for workflow construction.
