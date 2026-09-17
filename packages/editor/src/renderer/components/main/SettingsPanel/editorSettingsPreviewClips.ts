import noteEffectClip from '@assets/mp4/note-effect.mp4';
import keyCounterClip from '@assets/mp4/key-counter.mp4';
import customCssClip from '@assets/mp4/custom-css.mp4';
import customJsClip from '@assets/mp4/custom-js.mp4';
import type { PreviewClip } from './SettingsPreview';

export const EDITOR_SETTINGS_PREVIEW_CLIPS: Record<string, PreviewClip> = {
  noteEffect: { src: noteEffectClip, caption: 'settings.noteEffectDesc' },
  keyCounter: { src: keyCounterClip, caption: 'settings.keyCounterDesc' },
  customCSS: { src: customCssClip, caption: 'settings.customCSSDesc' },
  customJS: { src: customJsClip, caption: 'settings.customJSDesc' },
};
