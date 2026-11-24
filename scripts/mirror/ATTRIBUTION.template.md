# Attribution

## This copy

- Model: {model}
- Publisher: {publisher}
- Upstream repository: {upstream_repo_url}
- Upstream commit: {upstream_commit}
- Licence: {licence_name}
- Licence text: {licence_url}
- Converter: handy-computer, converted at commit {sha} to GGUF for transcribe.cpp, https://github.com/handy-computer/transcribe.cpp
- GGUF file: {mirrored_filename}

Redistributed unmodified by Coco Research.

"Unmodified" here means the GGUF bytes are identity-checked against handy-computer's file: same sha256, no re-quantizing, no metadata edits. `LICENSE` and `NOTICE` are copied from handy-computer's repository and are not rewritten.

## Licence blocks

Keep the model block above, add the block that matches the licence, then delete the others and this heading.

### MIT

Copyright notice: {copyright_notice}

`LICENSE` holds the MIT text and that copyright line, copied unchanged from handy-computer's repository. MIT requires the copyright notice and the permission notice to stay with every copy of the model, which is why they are shipped here.

Use this paragraph for Whisper repositories whose upstream card declares MIT, `whisper-large-v3-turbo` included.

### Apache-2.0

`LICENSE` holds the Apache-2.0 text, copied unchanged from handy-computer's repository. Apache-2.0 section 4 requires the licence text and any NOTICE file to travel with the work. `NOTICE` is handy-computer's file.

The copyright line inside `LICENSE` is the unfilled template line `Copyright [yyyy] [name of copyright owner]`. Leave it exactly as it is. The real owner is named above, in this file, not in `LICENSE`.

Apache-2.0 section 6 covers trademark: the publisher name above is used only to identify the model and does not suggest endorsement.

### CC-BY-4.0

- Creator: {publisher}
- Copyright notice as upstream states it: {copyright_notice}
- Licence: CC-BY-4.0, https://creativecommons.org/licenses/by/4.0/
- Original repository: {upstream_repo_url}, commit {upstream_commit}
- Converted to GGUF by handy-computer; this is a modified format of the original.

The modified-format line sits next to the "Redistributed unmodified" line on purpose: the GGUF file is a different format from the original weights, and the bytes Coco redistributes are the unmodified output of that conversion.

The Creative Commons licence's disclaimer of warranties applies to this model as it does to the original. It is provided as is, with no warranty of any kind.

This repository is not gated and carries no DRM.

### Whisper (all 12 Whisper models, whatever the card metadata says)

Ship this notice alongside whatever the Hugging Face card for that model declares:

Copyright (c) 2022 OpenAI

OpenAI's notice comes from https://github.com/openai/whisper, whose README says "Whisper's code and model weights are released under the MIT License." Some Whisper cards declare `apache-2.0` in their metadata and the upstream repository states MIT. Both are permissive, and the notice travels either way. `whisper-large-v3-turbo` is MIT in the upstream card and in handy-computer's LICENSE, so its block above is also MIT.

### Qwen3-4B (Qwen/Qwen3-4B-GGUF)

- Publisher: Qwen
- Upstream repository: https://huggingface.co/Qwen/Qwen3-4B-GGUF
- Upstream commit: bc640142c66e1fdd12af0bd68f40445458f3869b
- Licence: Apache-2.0, https://www.apache.org/licenses/LICENSE-2.0
- Copyright: Copyright 2025 Alibaba Cloud

Drop the converter line from "This copy". There is no handy-computer conversion and no NOTICE for this model. Do not write one.

Card sentence for this model, and only for this model:

Unofficial mirror of Qwen3-4B-GGUF by Qwen, as published by Qwen. Not affiliated with or endorsed by Qwen.

## Template notes, delete before publishing

- `{model}` is the model name as the publisher writes it, without the organisation prefix.
- `{publisher}` is the plain publisher name. The same name appears in the card's "not affiliated with or endorsed by" sentence.
- `{sha}` is the commit named in handy-computer's NOTICE as the one it converted from. It is the same value as `{upstream_commit}` for every model whose NOTICE and title pin agree. Fill both from the NOTICE, not from memory.
- `{copyright_notice}` is the copyright line exactly as the publisher states it. If the publisher states none, write `{UNKNOWN: the copyright notice as upstream states it}` and leave it visible. Do not invent an owner.
- `{licence_url}` is the canonical licence page. MIT: https://opensource.org/license/mit. Apache-2.0: https://www.apache.org/licenses/LICENSE-2.0. CC-BY-4.0: https://creativecommons.org/licenses/by/4.0/.
- Keep the line "Redistributed unmodified by Coco Research." word for word.
- Do not use publisher logos, the word "official", the phrase "Coco models", or any donate or sponsor link.
