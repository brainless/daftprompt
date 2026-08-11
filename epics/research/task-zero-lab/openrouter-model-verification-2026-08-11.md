# OpenRouter helper model verification — 2026-08-11

This artifact verifies the three discovery suggestions before any paid
completion. The OpenRouter catalog's `inferred_parameters_b` values are
non-authoritative discovery hints. Tier assignment below instead uses the
upstream model repositories and their published weight metadata/model cards.

| OpenRouter model ID | Upstream weight repository | Authoritative size evidence | Published license | Decision |
|---|---|---|---|---|
| `ibm-granite/granite-4.1-8b` | [`ibm-granite/granite-4.1-8b`](https://huggingface.co/ibm-granite/granite-4.1-8b) | IBM's model card calls it an 8B-parameter dense model and its architecture table lists 8B. | Apache-2.0 | Keep: open weights, below 10B. |
| `meta-llama/llama-3.1-8b-instruct` | [`meta-llama/Meta-Llama-3.1-8B-Instruct`](https://huggingface.co/meta-llama/Meta-Llama-3.1-8B-Instruct) | Meta's model card identifies the 8B model; the repository publishes its weight files. | [Llama 3.1 Community License](https://huggingface.co/meta-llama/Llama-3.1-8B-Instruct/blob/main/LICENSE), not Apache/MIT and not asserted here to be OSI open source. | Keep: publicly downloadable open weights, below 10B; preserve custom-license distinction. |
| `mistralai/mistral-nemo` | [`mistralai/Mistral-Nemo-Instruct-2407`](https://huggingface.co/mistralai/Mistral-Nemo-Instruct-2407) | Hugging Face weight metadata reports 12,247,782,400 BF16 parameters (12B display tier); the model card publishes the architecture and weights. | Apache-2.0 | Keep: open weights, 10B-to-below-20B tier. |

The three exact OpenRouter IDs therefore meet the intended tier distribution:
two distinct 8B helpers and one 12B helper. This verifies weight availability,
parameter tier, and repository-declared license only. It does not verify that
OpenRouter will route a later request to a particular upstream provider, or
that the provider will support every requested routing control at run time.

Sources were read on 2026-08-11. The credential-free catalog report is
`openrouter-candidates-2026-08-11.json`; its SHA-256 is recorded in the Epic
014 experiment note so the JSON artifact does not contain a self-referential
hash.
