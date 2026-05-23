# Mobile Generate Parity Design

## Goal

Make mobile Generate track desktop Generate automatically so new desktop Generate features show up on mobile without separate feature wiring.

## Constraints

- Mobile shell is only intended for browser-mode clients (LAN/server access), not local Tauri desktop usage.
- Mobile should remain usable on small screens while reusing desktop assets/components as much as possible.
- Avoid a long-lived fork between desktop and mobile Generate UIs.

## Selected Approach

Use a single shared Generate surface by rendering `GenerationPage` inside the mobile Generate route, and enable mobile-friendly behavior through component props instead of duplicating section implementations.

## Architecture

1. `MobileGeneratePage` becomes a thin wrapper around `GenerationPage`.
2. `GenerationPage` gains a `mobileFriendly` prop.
3. `GenerationPage` uses mobile-specific localStorage keys when `mobileFriendly` is true so desktop panel layouts are not overwritten by phone usage.
4. `GenerationPage` starts with side panels collapsed in mobile mode while keeping all desktop controls available.

## Data Flow

- Existing stores (`generation`, `canvas`, `gallery`, `models`, etc.) remain unchanged.
- Existing desktop component tree remains source-of-truth for Generate sections.
- Mobile route reuses the exact same section rendering, visibility logic, and controls through `GenerationPage`.

## Error Handling

- No new backend interfaces.
- Existing upload/drag/drop/interrogate error handling paths in `GenerationPage` are reused.

## Validation

- Open app in browser mode on mobile viewport and confirm Generate tab renders `GenerationPage`.
- Confirm side panels start collapsed and can be expanded.
- Confirm desktop app retains its own panel layout state after mobile interactions.
- Confirm Generate features previously missing on mobile are now present.
