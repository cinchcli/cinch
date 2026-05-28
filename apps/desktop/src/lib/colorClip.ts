export interface ParsedColorClip {
  value: string;
  cssColor: string;
  format: 'hex' | 'rgb' | 'hsl' | 'named' | 'css-function';
}

const HEX_COLOR_RE = /^#(?:[0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$/i;
const COLOR_FUNCTION_RE = /^(?:lab|lch|oklab|oklch|color)\(.+\)$/i;
const CSS_WIDE_KEYWORDS = new Set([
  'currentcolor',
  'inherit',
  'initial',
  'revert',
  'revert-layer',
  'unset',
]);

const NAMED_COLORS = new Set([
  'aliceblue', 'antiquewhite', 'aqua', 'aquamarine', 'azure',
  'beige', 'bisque', 'black', 'blanchedalmond', 'blue', 'blueviolet',
  'brown', 'burlywood', 'cadetblue', 'chartreuse', 'chocolate', 'coral',
  'cornflowerblue', 'cornsilk', 'crimson', 'cyan', 'darkblue', 'darkcyan',
  'darkgoldenrod', 'darkgray', 'darkgreen', 'darkgrey', 'darkkhaki',
  'darkmagenta', 'darkolivegreen', 'darkorange', 'darkorchid', 'darkred',
  'darksalmon', 'darkseagreen', 'darkslateblue', 'darkslategray',
  'darkslategrey', 'darkturquoise', 'darkviolet', 'deeppink', 'deepskyblue',
  'dimgray', 'dimgrey', 'dodgerblue', 'firebrick', 'floralwhite',
  'forestgreen', 'fuchsia', 'gainsboro', 'ghostwhite', 'gold', 'goldenrod',
  'gray', 'green', 'greenyellow', 'grey', 'honeydew', 'hotpink',
  'indianred', 'indigo', 'ivory', 'khaki', 'lavender', 'lavenderblush',
  'lawngreen', 'lemonchiffon', 'lightblue', 'lightcoral', 'lightcyan',
  'lightgoldenrodyellow', 'lightgray', 'lightgreen', 'lightgrey',
  'lightpink', 'lightsalmon', 'lightseagreen', 'lightskyblue',
  'lightslategray', 'lightslategrey', 'lightsteelblue', 'lightyellow',
  'lime', 'limegreen', 'linen', 'magenta', 'maroon', 'mediumaquamarine',
  'mediumblue', 'mediumorchid', 'mediumpurple', 'mediumseagreen',
  'mediumslateblue', 'mediumspringgreen', 'mediumturquoise',
  'mediumvioletred', 'midnightblue', 'mintcream', 'mistyrose', 'moccasin',
  'navajowhite', 'navy', 'oldlace', 'olive', 'olivedrab', 'orange',
  'orangered', 'orchid', 'palegoldenrod', 'palegreen', 'paleturquoise',
  'palevioletred', 'papayawhip', 'peachpuff', 'peru', 'pink', 'plum',
  'powderblue', 'purple', 'rebeccapurple', 'red', 'rosybrown',
  'royalblue', 'saddlebrown', 'salmon', 'sandybrown', 'seagreen',
  'seashell', 'sienna', 'silver', 'skyblue', 'slateblue', 'slategray',
  'slategrey', 'snow', 'springgreen', 'steelblue', 'tan', 'teal',
  'thistle', 'tomato', 'transparent', 'turquoise', 'violet', 'wheat',
  'white', 'whitesmoke', 'yellow', 'yellowgreen',
]);

export function parseColorClip(content: string): ParsedColorClip | null {
  const value = content.trim();
  if (!value || value.length > 128 || /[\r\n]/.test(value)) return null;

  const lower = value.toLowerCase();
  if (CSS_WIDE_KEYWORDS.has(lower)) return null;

  if (HEX_COLOR_RE.test(value)) {
    return { value, cssColor: value, format: 'hex' };
  }

  if (isRgbColor(value)) {
    return { value, cssColor: value, format: 'rgb' };
  }

  if (isHslColor(value)) {
    return { value, cssColor: value, format: 'hsl' };
  }

  if (NAMED_COLORS.has(lower)) {
    return { value, cssColor: lower, format: 'named' };
  }

  if (COLOR_FUNCTION_RE.test(value) && supportsCssColor(value)) {
    return { value, cssColor: value, format: 'css-function' };
  }

  return null;
}

function isRgbColor(value: string): boolean {
  const match = value.match(/^rgba?\(\s*(.+)\s*\)$/i);
  if (!match) return false;

  const parsed = splitColorFunctionArgs(match[1]);
  if (!parsed || parsed.channels.length !== 3) return false;
  if (!parsed.channels.every(isRgbChannel)) return false;
  return parsed.alpha === undefined || isAlphaChannel(parsed.alpha);
}

function isHslColor(value: string): boolean {
  const match = value.match(/^hsla?\(\s*(.+)\s*\)$/i);
  if (!match) return false;

  const parsed = splitColorFunctionArgs(match[1]);
  if (!parsed || parsed.channels.length !== 3) return false;
  const [hue, saturation, lightness] = parsed.channels;
  if (!isHue(hue) || !isPercentChannel(saturation) || !isPercentChannel(lightness)) return false;
  return parsed.alpha === undefined || isAlphaChannel(parsed.alpha);
}

function splitColorFunctionArgs(input: string): { channels: string[]; alpha?: string } | null {
  const trimmed = input.trim();
  if (!trimmed) return null;

  if (trimmed.includes(',')) {
    const parts = trimmed.split(',').map((part) => part.trim()).filter(Boolean);
    if (parts.length !== 3 && parts.length !== 4) return null;
    return { channels: parts.slice(0, 3), alpha: parts[3] };
  }

  const slashParts = trimmed.split('/').map((part) => part.trim());
  if (slashParts.length > 2) return null;
  const [channelsPart, alphaPart] = slashParts;
  const channels = channelsPart.split(/\s+/).filter(Boolean);
  if (channels.length !== 3) return null;
  return { channels, alpha: alphaPart || undefined };
}

function isRgbChannel(value: string): boolean {
  if (value.endsWith('%')) return isPercentChannel(value);
  const number = Number(value);
  return Number.isFinite(number) && number >= 0 && number <= 255;
}

function isPercentChannel(value: string): boolean {
  const number = Number(value.replace(/%$/, ''));
  return value.endsWith('%') && Number.isFinite(number) && number >= 0 && number <= 100;
}

function isAlphaChannel(value: string): boolean {
  if (value.endsWith('%')) return isPercentChannel(value);
  const number = Number(value);
  return Number.isFinite(number) && number >= 0 && number <= 1;
}

function isHue(value: string): boolean {
  return /^[-+]?(?:\d+|\d*\.\d+)(?:deg|grad|rad|turn)?$/i.test(value);
}

function supportsCssColor(value: string): boolean {
  return typeof CSS !== 'undefined' && typeof CSS.supports === 'function' && CSS.supports('color', value);
}
