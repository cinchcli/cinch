export const CLIP_FILTERS = ['all', 'text', 'image', 'code', 'url'] as const;

export type ClipFilter = typeof CLIP_FILTERS[number];
