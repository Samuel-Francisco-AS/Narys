// Shared configuration contract; importing it does not load the avatar runtime.
export type RenderBudgetConfig = { activeFps: number; backgroundFps: number }
export const defaultRenderBudgetConfig: RenderBudgetConfig = { activeFps: 30, backgroundFps: 24 }
