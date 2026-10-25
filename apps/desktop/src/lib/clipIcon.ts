import type { LocalClip } from '../bindings';

export type AgentCommand = 'claude' | 'codex' | 'pi';

// Matches one-line commands such as `claude --resume <id>`, `codex resume <id>`
// or `pi --session <path>`. The text is only classified, never executed.
const AGENT_COMMAND_RE = /^(?:\$\s+)?(claude|codex|pi)(?:\s|$)/;

// The login window owns focus while the screen is locked, so it is never the
// real origin of a copy. Clips attributed to it fall back to the type glyph.
const UNATTRIBUTABLE_APP_IDS = new Set(['com.apple.loginwindow']);

export function detectAgentCommand(content: string): AgentCommand | null {
  const text = content.trim();
  if (!text || text.includes('\n')) return null;
  const match = AGENT_COMMAND_RE.exec(text);
  return match ? (match[1] as AgentCommand) : null;
}

export function sourceAppIconId(clip: Pick<LocalClip, 'source_app_id'>): string | null {
  const id = clip.source_app_id;
  if (!id || UNATTRIBUTABLE_APP_IDS.has(id)) return null;
  return id;
}

export type ClipIcon =
  | { kind: 'agent'; agent: AgentCommand }
  | { kind: 'app'; appId: string }
  | { kind: 'type' };

export function clipIcon(clip: Pick<LocalClip, 'content' | 'content_type' | 'source_app_id'>): ClipIcon {
  if (clip.content_type === 'text' || clip.content_type === 'code') {
    const agent = detectAgentCommand(clip.content);
    if (agent) return { kind: 'agent', agent };
  }
  const appId = sourceAppIconId(clip);
  return appId ? { kind: 'app', appId } : { kind: 'type' };
}
