const MAX_TITLE_LEN = 60;

export interface TitleableClip {
  content: string;
  content_type: string;
  byte_size: number;
  source: string;
}

function formatBytes(b: number): string {
  if (b < 1024) return `${b} B`;
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`;
  return `${(b / (1024 * 1024)).toFixed(1)} MB`;
}

export function clipTitle(clip: TitleableClip): string {
  if (clip.content_type === 'image') {
    return `Image (${formatBytes(clip.byte_size)})`;
  }
  const collapsed = clip.content.replace(/\s+/g, ' ').trim();
  if (!collapsed) return '(empty clip)';
  if (collapsed.length <= MAX_TITLE_LEN) return collapsed;
  return collapsed.slice(0, MAX_TITLE_LEN) + '…';
}
