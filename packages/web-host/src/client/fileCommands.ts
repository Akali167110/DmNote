import type { WebFile } from '../protocol';
import type { FilePickerOptions } from '../browser/files';

export type BrowserFilePicker = (
  options: FilePickerOptions,
) => Promise<WebFile[]>;

const fileCommands: Record<string, { accept: string; multiple?: boolean }> = {
  preset_load: { accept: '.json,application/json' },
  preset_load_tab: { accept: '.json,application/json' },
  image_load: { accept: '.png,.jpg,.jpeg,.gif,.webp,.avif,.bmp,.svg,image/*' },
  font_load: { accept: '.ttf,.otf,.woff,.woff2' },
  sound_load: { accept: '.wav,.mp3,.ogg,.flac,.aac,.m4a,audio/*' },
  css_load: { accept: '.css,text/css' },
  css_tab_load: { accept: '.css,text/css' },
  js_load: { accept: '.js,.mjs,text/javascript', multiple: true },
};

export type PreparedFileCommand =
  | { cancelled: true; result: { success: false; tabId?: string } }
  | { cancelled: false; args: Record<string, unknown>; files?: WebFile[] };

export async function prepareFileCommand(
  command: string,
  args: Record<string, unknown>,
  picker: BrowserFilePicker,
  signal: AbortSignal,
): Promise<PreparedFileCommand> {
  const options = fileCommands[command];
  if (!options) return { cancelled: false, args };
  const files = await picker({ ...options, signal });
  if (!files.length)
    return {
      cancelled: true,
      result: {
        success: false,
        ...(command === 'css_tab_load' ? { tabId: String(args.tabId) } : {}),
      },
    };
  if (command === 'preset_load' || command === 'preset_load_tab') {
    const preset: unknown = JSON.parse(
      new TextDecoder().decode(files[0].bytes),
    );
    return { cancelled: false, args: { ...args, preset } };
  }
  return { cancelled: false, args, files };
}
