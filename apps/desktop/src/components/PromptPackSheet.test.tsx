import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import type { LocalClip, PromptRecipeDto } from '../bindings';
import { PromptPackSheet } from './PromptPackSheet';

const recipes: PromptRecipeDto[] = [
  {
    id: 'better-final-answer',
    label: 'Better Final Answer',
    description: 'Combine answers into one stronger response.',
  },
  {
    id: 'html-mockup',
    label: 'HTML Mockup',
    description: 'Create a self-contained HTML mockup.',
  },
];

const clip = (id: string, content: string): LocalClip => ({
  id,
  user_id: 'u1',
  content,
  content_type: 'text',
  source: 'local',
  label: '',
  byte_size: content.length,
  media_path: null,
  created_at: 1_777_614_529,
  synced: true,
  sync_state: 'synced',
  is_pinned: false,
  pin_note: null,
  received_at: 1_777_614_529,
});

describe('PromptPackSheet', () => {
  it('filters recipes and builds with selected context clips', () => {
    const onBuild = vi.fn();
    render(
      <PromptPackSheet
        primaryClip={clip('primary', 'latest copied text')}
        clips={[clip('primary', 'latest copied text'), clip('ctx1', 'second answer')]}
        recipes={recipes}
        onBuild={onBuild}
        onClose={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByLabelText(/second answer/i));
    fireEvent.change(screen.getByRole('textbox', { name: /prompt pack/i }), {
      target: { value: 'html' },
    });

    expect(screen.getByText('HTML Mockup')).toBeInTheDocument();
    expect(screen.queryByText('Better Final Answer')).not.toBeInTheDocument();

    fireEvent.keyDown(screen.getByRole('textbox', { name: /prompt pack/i }), { key: 'Enter' });

    expect(onBuild).toHaveBeenCalledWith('html-mockup', ['ctx1']);
  });

  it('closes on Escape', () => {
    const onClose = vi.fn();
    render(
      <PromptPackSheet
        primaryClip={clip('primary', 'latest copied text')}
        clips={[clip('primary', 'latest copied text')]}
        recipes={recipes}
        onBuild={vi.fn()}
        onClose={onClose}
      />,
    );

    fireEvent.keyDown(window, { key: 'Escape' });

    expect(onClose).toHaveBeenCalled();
  });
});
