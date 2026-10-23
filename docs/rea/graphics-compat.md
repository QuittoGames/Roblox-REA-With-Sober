# Compatibilidade gráfica Roblox/Sober — relatório técnico

**Projeto:** REA (`rea-gfx` / `hardness_gfx`)
**Data:** 2026-10-23
**Status:** tooling funcional; validação de sessão real em andamento

---

## 1. Overview

Ferramenta Rust de diagnóstico e validação do caminho gráfico do Roblox via
Sober (cliente Android executado sobre camada de compatibilidade Vulkan/Mesa
no host Linux). A ferramenta **observa e configura via variáveis de ambiente
públicas do Mesa**; nenhum binário do Sober, do cliente Roblox ou da stack
gráfica é modificado.

## 2. Evidências de baseline (FACT, coletadas via `rea-gfx doctor --json`)

### Host

| Item | Valor |
|---|---|
| OS | Fedora Linux 44 (Workstation Edition) |
| Kernel | 7.2.11-200.fc44.x86_64 |
| Sessão | GNOME Wayland (XWayland disponível) |
| CPU | Intel Core i5-6200U (SSE4.2 presente — emulação do libbadcpu inativa) |
| GPU | Intel HD 520 (SKL GT2), PCI 8086:1916, driver i915 |
| Mesa | 26.2.4, renderer "Mesa Intel(R) HD Graphics 520 (SKL GT2)" |
| Vulkan loader | 1.4.341 (`/usr/lib64/libvulkan.so.1.4.341`) |

### Sober

| Item | Valor |
|---|---|
| Ref | app/org.vinegarhq.Sober/x86_64/stable |
| Commit | 7b6430c703e8… |
| Runtime | org.gnome.Platform/x86_64/51 |
| Config | `use_opengl=false` (Vulkan), `graphics_optimization_mode=balanced` |

### Sonda Vulkan própria (rea-gfx probe)

- 2 dispositivos enumerados: Intel HD 520 (INTEGRATED_GPU) e llvmpipe (CPU).
- Intel: API 1.4.354, driver 26.2.4, memória unificada (1 heap 9,3 GB
  device_local+host_visible), 1 queue family graphics+compute+transfer.
- Surface XWayland (VK_KHR_xlib_surface) criada com sucesso:
  - formato `B8G8R8A8_SRGB`, color space `SRGB_NONLINEAR`
  - present modes: `IMMEDIATE, MAILBOX, FIFO, FIFO_RELAXED`
  - `min_image_count=3`, sem máximo (0)
- Observação (UNKNOWN): `queue_families[0].present=false` retornado para a
  surface de teste — possivelmente artefato da janela dummy 320x200; o probe
  hoje converte erro em `false` silenciosamente (`unwrap_or(false)`), o que
  deve ser corrigido para distinguir os dois casos.

### Pipeline cache (RPCV)

- Arquivo: `~/.var/app/org.vinegarhq.Sober/cache/sober/shadercachevk.bin`
- Container `RPCV` versão 6422; header VkPipelineCache v1 válido
- Vendor/device do header: 0x8086/0x1916 — **match** com a GPU ativa (FACT)

## 3. Arquitetura da solução

```
rea-gfx (CLI Rust, ash 0.38)
├── host      → inventário do host (OS, GPU sysfs, Mesa, CPU flags)
├── sober     → descoberta da instalação Flatpak (paths, commit, logs)
├── config    → parser do config.json do Sober (com comentários //)
├── logs      → parser do log do Sober → fatos Vulkan estruturados
├── cache     → inspeção do container RPCV + header VkPipelineCache
├── profile   → profiles de present-mode (env vars públicas do Mesa)
├── run       → launcher flatpak com env vars por profile
├── vulkan    → sonda Vulkan/WSI independente (ash, surface Xlib)
└── doctor    → consolida tudo em relatório + checks + findings
```

Princípios:

1. **Não-invasivo**: somente leitura do estado do Sober; a única escrita é o
   relatório próprio. O lançamento usa `flatpak run --env=…` (escopo do
   processo filho, sem alterar config do Sober nem o host).
2. **Evidência antes de conclusão**: fatos extraídos dos logs são marcados;
   ausência de dado não é inventada (testes cobrem isso).
3. **Perfis documentados**: cada profile mapeia para valor documentado do
   Mesa (`MESA_VK_WSI_PRESENT_MODE=fifo|relaxed|mailbox|immediate`).

## 4. Validação do tooling

| Check | Resultado |
|---|---|
| `cargo fmt --check` | PASS |
| `cargo check` | PASS |
| `cargo test` | PASS (14/14) |
| `cargo clippy --all-targets` | PASS (0 warnings) |
| `doctor --json` runtime | PASS (exit 0, relatório completo) |

Correções registradas durante o desenvolvimento:

- Probe Vulkan exigia `VK_KHR_surface` **e** `VK_KHR_xlib_surface` no
  `enabled_extension_names` — sem o primeiro, os function pointers de
  surface não resolvem (panic do ash).
- Clippy: `manual_ok_err`, `format_in_format_args`, `manual_c_str_literals`.

## 5. Evidência de sessão de jogo (pré-tooling, INFERENCE forte)

Logs anteriores do Sober (analisados na fase de descoberta) registram sessão
real de jogo: `create_vulkan=true`, VK_KHR_surface+android_surface, swapchain
com 4 imagens, present mode FIFO, formato 44 (B8G8R8A8_UNORM), 1366x690,
2988 shaders compilados, pipeline cache 274 KB (1 execução pulada por
falha de validação). **O Roblox já rodou com sucesso neste host via Sober.**

## 6. Limitações e riscos conhecidos

- GPU SKL GT2 (2015, ~4% Vulkan 1.3 features parciais): performance é o
  risco principal, não compatibilidade binária.
- O `latest.log` atual (launcher, 1.1 KB) não contém fatos de sessão — o
  parser depende de log de sessão real (jogo aberto).
- Present support retornando `false` no probe dummy (ver §2) precisa de
  correção para reportar erro em vez de silenciar.

## 7. Próximos passos

1. Vertical slice #1: `verify --profile baseline` com sessão real → comparar
   fatos do log (present_mode, formato, swapchain) com a sonda do host.
2. Corrigir `unwrap_or(false)` do probe para distinguir erro de `false`.
3. Repetir com `--profile fifo` e comparar (FIFO é o valor que o Android
   client reporta por padrão — eliminar vsync duplo é hipótese a testar).
4. Decisão de profile default pertence ao DEV (ver `.agents/state.json`).

---

## 8. Vertical slice #1 — sessão real observada (2026-10-23)

**Método:** `rea-gfx verify --profile baseline --scenario vertical-slice-1`
com o DEV logado e entrando em um jogo real. Fatos extraídas do
`latest.log` do Sober (parse estruturado validado após correção do parser
para o formato atual).

### Fatos da sessão (FACT, via parser corrigido)

| Fato | Valor |
|---|---|
| create_vulkan / create_opengl | true / false |
| GPU selecionada | Intel(R) HD Graphics 520 (SKL GT2) |
| Vulkan API / driver | 1.4.354 / 26.2.5 |
| Memória unificada | true (heap 732 MB reportado; device memory 9.3 GB) |
| caps.videoMemory | 67108864 (64 MB — valor de capacidade do caminho de compat, não o heap real) |
| Swapchain | 4 imagens, present mode 2 (FIFO), format 44 (B8G8R8A8_UNORM), 1366x690 |
| Shader pack | vulkan_mobile (2991 carregados) |
| Shaders compilados em runtime | 1079 em 529 ms |
| Pipeline cache | 274265 bytes — **PULADO** ("validation failed") |
| llvmpipe | corretamente ignorado (emulado) |
| Estabilidade | sessão > 8 min sem crash; swapchain recriado 1x no boot, depois estável |

### Feedback do DEV durante a sessão (registrado verbatim)

> "to em um pc sem GPU da amd"
> "esta bem lagado porem acredito que pode ser pelo propio pc , me parece que
> os graficos esto mais limpos mais n cosigo dizer , e me parece ate que
> relativennte estavel apra meu processador prosiga com as imple e o REA para
> perfesusar"

Classificação: percepção de lag/estabilidade = INFERENCE do DEV (coerente
com a evidência de estabilidade objetiva). "Gráficos mais limpos" não é
confirmável por este teste (o perfil baseline não aplica override de
qualidade; DRS dinâmico pode explicar variação).

## 9. Causa raiz do skip do pipeline cache (FACT, confirmada por probe)

O cross-check entre o UUID embutido no header RPCV e o `pipelineCacheUUID`
ao vivo do device:

| Fonte | UUID |
|---|---|
| Cache RPCV | `d96a0b113cd5ea81013a7becdfd911aa` |
| Device Intel atual | `2a4ae900fc21eea083ff35633036f583` |

**Mismatch.** O cache foi gerado por outra geração do driver (o host
recebeu update de Mesa 26.0.x → 26.2.x) e o Roblox o rejeita em toda
sessão → recompilação de ~1079 shaders a cada boot (hitching inicial +
pipelines novos durante o gameplay).

**Correção candidata (decisão do DEV, pendente):** regenerar o cache
removendo (ou renomeando para backup) `shadercachevk.bin` — arquivo
inválido, regenerável, sem valor. A primeira sessão seguinte recompila e
grava cache novo válido; as subsequentes passam a reutilizá-lo.

## 10. Correções de tooling registradas nesta iteração

1. `cache.rs`: UUID do header lido do offset correto (`16..32` do
   VkPipelineCacheHeaderVersionOne; antes lia device_id + metade do UUID).
2. `logs.rs`: parser atualizado para o formato atual do log do Sober
   (`Vulkan Device: …`, `swapchain images 4 present mode 2 format 44`,
   `Loaded N shaders from pack …`, `Compiled N shaders in …`,
   `pipeline cache (N bytes)`); novo campo `shaders_compiled`; formato
   legado mantido.
3. `doctor.rs`: `verify` lê o stderr capturado quando o stdout vem vazio
   (o Sober escreve diagnóstico no stderr); finding de cross-check UUID
   cache↔device com classificação FACT e recomendação condicionada ao DEV.
4. `vulkan.rs`: `DeviceProbe.pipeline_cache_uuid` exposto (hex do
   `VkPhysicalDeviceProperties.pipelineCacheUUID`).

Validação: cargo fmt/check/test (17/17)/clippy — PASS.

## 11. Sessão com MangoHud + análise de integridade (2026-10-23, pós-regeneração)

Sessão de gameplay real com `MANGOHUD=1` (layer 26.08 instalado). Saída limpa
(exit 0). Fatos do log:

| Fato | Valor | vs. sessão pré-regeneração |
|---|---|---|
| Pipeline cache | **Loaded** 2.6 MB (hash c8ad65fe), Saved 2.6 MB | era **Skipping** toda sessão |
| Shaders carregados | 2991 (vulkan_mobile) | igual |
| Shaders compilados em runtime | 1079 em **284 ms** | 529 ms (≈2x pior) |
| Erros de render/shader | **0** | 0 |
| DRS | denied=false, canUseHighDPIFullFB=true | igual |
| Swapchain | 4 imgs, FIFO, format 44, 1366x690 | igual |

**Conclusão de integridade (FACT):** caminho gráfico íntegro e mais rápido;
o cache agora persiste entre sessões (2.6 MB de pipelines acumulados).
Observação do MangoHud/Sober sobre "third party software" é cosmética.

**Sobre "igual ao Windows" (FACT):** não atingível por arquitetura — o Sober
executa o client Android com pack `vulkan_mobile`; o client Windows usa D3D11
com shaders PC. Além disso o Hyperion bloqueia Wine intencionalmente (fator
que deprecou o Vinegar em 2024-02); bypass é fora de escopo (§1 Boundaries).
Rota legítima restante para qualidade: FFlags via config.json do Sober.

## 12. Viabilidade da "junção" Windows+Android (questionamento do DEV)

Resposta com evidência: **não viável**. Decompilação completa do client
Windows não existe publicamente; o Sober não traduz binários Windows (roda o
APK Android real); o Hyperion bloqueia Wine por decisão consciente da Roblox
("we made the conscious decision to block the use of wine"). O §10 do prompt
original prevê este desfecho: entregar a melhor implementação permitida da
alternativa validada — que é o H1 (diagnóstico + cache + profiles), concluído
neste repositório.
