import { useEffect, useState, type ComponentType } from 'react';

type IconComponent = ComponentType<{
  className?: string;
  size?: number | string;
  title?: string;
}>;

type AvatarComponent = ComponentType<{
  className?: string;
  shape?: 'circle' | 'square';
  size: number;
  title?: string;
}>;

type ProviderIconComponent = IconComponent & {
  Avatar?: AvatarComponent;
  BrandColor?: IconComponent;
  Color?: IconComponent;
};

export type ModelProviderIconTheme = 'mono' | 'logo' | 'brand';

type ProviderIconLoader = () => Promise<ProviderIconComponent>;

const providerIconLoaders = {
  Ai21: () =>
    import('@lobehub/icons/es/Ai21').then((module) => module.default as ProviderIconComponent),
  Ai2: () =>
    import('@lobehub/icons/es/Ai2').then((module) => module.default as ProviderIconComponent),
  AiStudio: () =>
    import('@lobehub/icons/es/AiStudio').then((module) => module.default as ProviderIconComponent),
  AionLabs: () =>
    import('@lobehub/icons/es/AionLabs').then((module) => module.default as ProviderIconComponent),
  Arcee: () =>
    import('@lobehub/icons/es/Arcee').then((module) => module.default as ProviderIconComponent),
  BAAI: () =>
    import('@lobehub/icons/es/BAAI').then((module) => module.default as ProviderIconComponent),
  Alibaba: () =>
    import('@lobehub/icons/es/Alibaba').then((module) => module.default as ProviderIconComponent),
  AlibabaCloud: () =>
    import('@lobehub/icons/es/AlibabaCloud').then(
      (module) => module.default as ProviderIconComponent,
    ),
  Anthropic: () =>
    import('@lobehub/icons/es/Anthropic').then((module) => module.default as ProviderIconComponent),
  Aws: () =>
    import('@lobehub/icons/es/Aws').then((module) => module.default as ProviderIconComponent),
  Baidu: () =>
    import('@lobehub/icons/es/Baidu').then((module) => module.default as ProviderIconComponent),
  Bailian: () =>
    import('@lobehub/icons/es/Bailian').then((module) => module.default as ProviderIconComponent),
  Baichuan: () =>
    import('@lobehub/icons/es/Baichuan').then((module) => module.default as ProviderIconComponent),
  ByteDance: () =>
    import('@lobehub/icons/es/ByteDance').then((module) => module.default as ProviderIconComponent),
  ChatGLM: () =>
    import('@lobehub/icons/es/ChatGLM').then((module) => module.default as ProviderIconComponent),
  Cloudflare: () =>
    import('@lobehub/icons/es/Cloudflare').then(
      (module) => module.default as ProviderIconComponent,
    ),
  Cohere: () =>
    import('@lobehub/icons/es/Cohere').then((module) => module.default as ProviderIconComponent),
  DeepInfra: () =>
    import('@lobehub/icons/es/DeepInfra').then((module) => module.default as ProviderIconComponent),
  DeepSeek: () =>
    import('@lobehub/icons/es/DeepSeek').then((module) => module.default as ProviderIconComponent),
  Doubao: () =>
    import('@lobehub/icons/es/Doubao').then((module) => module.default as ProviderIconComponent),
  EssentialAI: () =>
    import('@lobehub/icons/es/EssentialAI').then(
      (module) => module.default as ProviderIconComponent,
    ),
  Fireworks: () =>
    import('@lobehub/icons/es/Fireworks').then((module) => module.default as ProviderIconComponent),
  Google: () =>
    import('@lobehub/icons/es/Google').then((module) => module.default as ProviderIconComponent),
  GoogleCloud: () =>
    import('@lobehub/icons/es/GoogleCloud').then(
      (module) => module.default as ProviderIconComponent,
    ),
  Grok: () =>
    import('@lobehub/icons/es/Grok').then((module) => module.default as ProviderIconComponent),
  Groq: () =>
    import('@lobehub/icons/es/Groq').then((module) => module.default as ProviderIconComponent),
  HuggingFace: () =>
    import('@lobehub/icons/es/HuggingFace').then(
      (module) => module.default as ProviderIconComponent,
    ),
  Huawei: () =>
    import('@lobehub/icons/es/Huawei').then((module) => module.default as ProviderIconComponent),
  IBM: () =>
    import('@lobehub/icons/es/IBM').then((module) => module.default as ProviderIconComponent),
  IFlyTekCloud: () =>
    import('@lobehub/icons/es/IFlyTekCloud').then(
      (module) => module.default as ProviderIconComponent,
    ),
  Inflection: () =>
    import('@lobehub/icons/es/Inflection').then(
      (module) => module.default as ProviderIconComponent,
    ),
  InternLM: () =>
    import('@lobehub/icons/es/InternLM').then((module) => module.default as ProviderIconComponent),
  Jina: () =>
    import('@lobehub/icons/es/Jina').then((module) => module.default as ProviderIconComponent),
  KwaiKAT: () =>
    import('@lobehub/icons/es/KwaiKAT').then((module) => module.default as ProviderIconComponent),
  Kwaipilot: () =>
    import('@lobehub/icons/es/Kwaipilot').then((module) => module.default as ProviderIconComponent),
  Liquid: () =>
    import('@lobehub/icons/es/Liquid').then((module) => module.default as ProviderIconComponent),
  Meta: () =>
    import('@lobehub/icons/es/Meta').then((module) => module.default as ProviderIconComponent),
  Microsoft: () =>
    import('@lobehub/icons/es/Microsoft').then((module) => module.default as ProviderIconComponent),
  Minimax: () =>
    import('@lobehub/icons/es/Minimax').then((module) => module.default as ProviderIconComponent),
  Mistral: () =>
    import('@lobehub/icons/es/Mistral').then((module) => module.default as ProviderIconComponent),
  Moonshot: () =>
    import('@lobehub/icons/es/Moonshot').then((module) => module.default as ProviderIconComponent),
  Morph: () =>
    import('@lobehub/icons/es/Morph').then((module) => module.default as ProviderIconComponent),
  Nebius: () =>
    import('@lobehub/icons/es/Nebius').then((module) => module.default as ProviderIconComponent),
  Nvidia: () =>
    import('@lobehub/icons/es/Nvidia').then((module) => module.default as ProviderIconComponent),
  NousResearch: () =>
    import('@lobehub/icons/es/NousResearch').then(
      (module) => module.default as ProviderIconComponent,
    ),
  Ollama: () =>
    import('@lobehub/icons/es/Ollama').then((module) => module.default as ProviderIconComponent),
  OpenAI: () =>
    import('@lobehub/icons/es/OpenAI').then((module) => module.default as ProviderIconComponent),
  OpenRouter: () =>
    import('@lobehub/icons/es/OpenRouter').then(
      (module) => module.default as ProviderIconComponent,
    ),
  Perplexity: () =>
    import('@lobehub/icons/es/Perplexity').then(
      (module) => module.default as ProviderIconComponent,
    ),
  Qwen: () =>
    import('@lobehub/icons/es/Qwen').then((module) => module.default as ProviderIconComponent),
  Relace: () =>
    import('@lobehub/icons/es/Relace').then((module) => module.default as ProviderIconComponent),
  SambaNova: () =>
    import('@lobehub/icons/es/SambaNova').then((module) => module.default as ProviderIconComponent),
  SiliconCloud: () =>
    import('@lobehub/icons/es/SiliconCloud').then(
      (module) => module.default as ProviderIconComponent,
    ),
  Stepfun: () =>
    import('@lobehub/icons/es/Stepfun').then((module) => module.default as ProviderIconComponent),
  Tencent: () =>
    import('@lobehub/icons/es/Tencent').then((module) => module.default as ProviderIconComponent),
  Together: () =>
    import('@lobehub/icons/es/Together').then((module) => module.default as ProviderIconComponent),
  Upstage: () =>
    import('@lobehub/icons/es/Upstage').then((module) => module.default as ProviderIconComponent),
  VertexAI: () =>
    import('@lobehub/icons/es/VertexAI').then((module) => module.default as ProviderIconComponent),
  Venice: () =>
    import('@lobehub/icons/es/Venice').then((module) => module.default as ProviderIconComponent),
  Voyage: () =>
    import('@lobehub/icons/es/Voyage').then((module) => module.default as ProviderIconComponent),
  Wenxin: () =>
    import('@lobehub/icons/es/Wenxin').then((module) => module.default as ProviderIconComponent),
  XAI: () =>
    import('@lobehub/icons/es/XAI').then((module) => module.default as ProviderIconComponent),
  XiaomiMiMo: () =>
    import('@lobehub/icons/es/XiaomiMiMo').then(
      (module) => module.default as ProviderIconComponent,
    ),
  Yi: () =>
    import('@lobehub/icons/es/Yi').then((module) => module.default as ProviderIconComponent),
  ZAI: () =>
    import('@lobehub/icons/es/ZAI').then((module) => module.default as ProviderIconComponent),
  ZeroOne: () =>
    import('@lobehub/icons/es/ZeroOne').then((module) => module.default as ProviderIconComponent),
  Zhipu: () =>
    import('@lobehub/icons/es/Zhipu').then((module) => module.default as ProviderIconComponent),
} satisfies Record<string, ProviderIconLoader>;

const providerIcons: Array<{ aliases: string[]; loader: ProviderIconLoader }> = [
  { aliases: ['openai', '~openai'], loader: providerIconLoaders.OpenAI },
  { aliases: ['anthropic', '~anthropic'], loader: providerIconLoaders.Anthropic },
  { aliases: ['google', 'google-ai', 'gemini', '~google'], loader: providerIconLoaders.Google },
  { aliases: ['google-cloud'], loader: providerIconLoaders.GoogleCloud },
  { aliases: ['vertex-ai', 'vertex', 'google-vertex'], loader: providerIconLoaders.VertexAI },
  { aliases: ['google-ai-studio', 'ai-studio'], loader: providerIconLoaders.AiStudio },
  { aliases: ['deepseek'], loader: providerIconLoaders.DeepSeek },
  { aliases: ['x-ai', 'xai'], loader: providerIconLoaders.XAI },
  { aliases: ['z-ai', 'zai'], loader: providerIconLoaders.ZAI },
  { aliases: ['grok'], loader: providerIconLoaders.Grok },
  { aliases: ['meta', 'meta-ai', 'meta-llama', 'llama'], loader: providerIconLoaders.Meta },
  { aliases: ['mistral', 'mistralai'], loader: providerIconLoaders.Mistral },
  {
    aliases: ['moonshot', 'moonshotai', 'kimi', '~moonshotai'],
    loader: providerIconLoaders.Moonshot,
  },
  { aliases: ['qwen'], loader: providerIconLoaders.Qwen },
  { aliases: ['alibaba'], loader: providerIconLoaders.Alibaba },
  { aliases: ['alibaba-cloud'], loader: providerIconLoaders.AlibabaCloud },
  { aliases: ['bailian'], loader: providerIconLoaders.Bailian },
  { aliases: ['baichuan'], loader: providerIconLoaders.Baichuan },
  { aliases: ['cohere'], loader: providerIconLoaders.Cohere },
  { aliases: ['perplexity'], loader: providerIconLoaders.Perplexity },
  { aliases: ['openrouter'], loader: providerIconLoaders.OpenRouter },
  { aliases: ['groq'], loader: providerIconLoaders.Groq },
  { aliases: ['huggingface'], loader: providerIconLoaders.HuggingFace },
  { aliases: ['nvidia'], loader: providerIconLoaders.Nvidia },
  {
    aliases: ['microsoft', 'azure', 'azure-ai', 'azure-openai'],
    loader: providerIconLoaders.Microsoft,
  },
  { aliases: ['aws', 'bedrock', 'amazon'], loader: providerIconLoaders.Aws },
  { aliases: ['cloudflare', 'workers-ai'], loader: providerIconLoaders.Cloudflare },
  { aliases: ['baidu', 'baidu-cloud', 'wenxin'], loader: providerIconLoaders.Baidu },
  { aliases: ['ernie'], loader: providerIconLoaders.Wenxin },
  { aliases: ['huawei', 'huawei-cloud'], loader: providerIconLoaders.Huawei },
  { aliases: ['tencent', 'tencent-cloud', 'hunyuan'], loader: providerIconLoaders.Tencent },
  { aliases: ['ollama'], loader: providerIconLoaders.Ollama },
  { aliases: ['doubao'], loader: providerIconLoaders.Doubao },
  { aliases: ['volcengine', 'bytedance', 'bytedance-seed'], loader: providerIconLoaders.ByteDance },
  { aliases: ['chatglm', 'zhipu'], loader: providerIconLoaders.Zhipu },
  { aliases: ['glm', 'glmv'], loader: providerIconLoaders.ChatGLM },
  { aliases: ['deepinfra'], loader: providerIconLoaders.DeepInfra },
  { aliases: ['fireworks'], loader: providerIconLoaders.Fireworks },
  { aliases: ['iflytek-cloud', 'iflytek', 'spark'], loader: providerIconLoaders.IFlyTekCloud },
  { aliases: ['internlm', 'infinigence'], loader: providerIconLoaders.InternLM },
  { aliases: ['jina'], loader: providerIconLoaders.Jina },
  { aliases: ['minimax'], loader: providerIconLoaders.Minimax },
  { aliases: ['nebius'], loader: providerIconLoaders.Nebius },
  { aliases: ['nousresearch', 'nous-research'], loader: providerIconLoaders.NousResearch },
  { aliases: ['sambanova'], loader: providerIconLoaders.SambaNova },
  { aliases: ['siliconcloud', 'silicon-cloud'], loader: providerIconLoaders.SiliconCloud },
  { aliases: ['stepfun', 'step'], loader: providerIconLoaders.Stepfun },
  { aliases: ['together'], loader: providerIconLoaders.Together },
  { aliases: ['upstage'], loader: providerIconLoaders.Upstage },
  { aliases: ['voyage'], loader: providerIconLoaders.Voyage },
  { aliases: ['yi', 'lingyiwanwu'], loader: providerIconLoaders.Yi },
  { aliases: ['01-ai', 'zeroone', 'zero-one'], loader: providerIconLoaders.ZeroOne },
  { aliases: ['ai21'], loader: providerIconLoaders.Ai21 },
  { aliases: ['allenai'], loader: providerIconLoaders.Ai2 },
  { aliases: ['aion-labs'], loader: providerIconLoaders.AionLabs },
  { aliases: ['arcee-ai'], loader: providerIconLoaders.Arcee },
  { aliases: ['baai'], loader: providerIconLoaders.BAAI },
  { aliases: ['essentialai', 'essential-ai'], loader: providerIconLoaders.EssentialAI },
  { aliases: ['ibm-granite', 'ibm'], loader: providerIconLoaders.IBM },
  { aliases: ['inflection'], loader: providerIconLoaders.Inflection },
  { aliases: ['kwaipilot'], loader: providerIconLoaders.Kwaipilot },
  { aliases: ['kwaivgi'], loader: providerIconLoaders.KwaiKAT },
  { aliases: ['liquid'], loader: providerIconLoaders.Liquid },
  { aliases: ['morph'], loader: providerIconLoaders.Morph },
  { aliases: ['relace'], loader: providerIconLoaders.Relace },
  { aliases: ['venice'], loader: providerIconLoaders.Venice },
  { aliases: ['xiaomi', 'xiaomi-mimo'], loader: providerIconLoaders.XiaomiMiMo },
];

export function ModelProviderIcon({
  provider,
  size = 20,
  theme = 'mono',
}: {
  provider: string;
  size?: number;
  theme?: ModelProviderIconTheme;
}) {
  const normalizedProvider = normalizeProvider(provider);
  const loader = resolveProviderIconLoader(normalizedProvider);
  const [Icon, setIcon] = useState<ProviderIconComponent | null>(null);

  useEffect(() => {
    let cancelled = false;
    setIcon(null);

    if (!loader) {
      return () => {
        cancelled = true;
      };
    }

    void loader()
      .then((loadedIcon) => {
        if (!cancelled) {
          setIcon(() => loadedIcon);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setIcon(null);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [loader]);

  const Avatar = Icon?.Avatar;
  const ColorIcon = Icon?.Color ?? Icon?.BrandColor;
  const useBrandIcon = theme === 'brand' && Avatar;
  const LogoIcon = theme === 'logo' && ColorIcon ? ColorIcon : Icon;

  return (
    <span
      className={
        useBrandIcon
          ? 'models-provider-mark models-provider-mark-brand'
          : theme === 'logo' && ColorIcon
            ? 'models-provider-mark models-provider-mark-logo'
            : 'models-provider-mark'
      }
      aria-hidden="true"
    >
      {useBrandIcon ? (
        <Avatar
          className="models-provider-mark-avatar"
          shape="square"
          size={Math.max(size + 12, 30)}
          title={provider}
        />
      ) : LogoIcon ? (
        <LogoIcon className="models-provider-mark-svg" size={size} title={provider} />
      ) : (
        <span className="models-provider-mark-fallback">
          {providerFallbackText(normalizedProvider)}
        </span>
      )}
    </span>
  );
}

function resolveProviderIconLoader(provider: string) {
  return providerIcons.find(({ aliases }) => aliases.includes(provider))?.loader ?? null;
}

function normalizeProvider(provider: string) {
  return provider
    .trim()
    .toLowerCase()
    .replace(/^[~]+/, '')
    .replace(/[_\s]+/g, '-')
    .replace(/\/+/g, '-')
    .replace(/-+/g, '-');
}

function providerFallbackText(provider: string) {
  const parts = provider.split(/[-_/\s]+/).filter(Boolean);
  return (
    parts
      .slice(0, 2)
      .map((part) => part[0]?.toUpperCase() ?? '')
      .join('') || '?'
  );
}
