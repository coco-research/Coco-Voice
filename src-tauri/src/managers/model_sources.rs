//! Pinned publisher files for the eight legacy tarball models.
//! On-disk names match the old tar extraction, including gigaam renames.

use super::model::RemoteFile;

pub struct StaticFile {
    pub url: &'static str,
    pub local_name: &'static str,
    pub sha256: &'static str,
    pub size: u64,
}

pub fn files(specs: &[StaticFile]) -> Vec<RemoteFile> {
    specs
        .iter()
        .map(|s| RemoteFile {
            url: s.url.to_string(),
            local_name: s.local_name.to_string(),
            sha256: s.sha256.to_string(),
            size: s.size,
        })
        .collect()
}

pub fn total_mb(specs: &[StaticFile]) -> u64 {
    specs.iter().map(|s| s.size).sum::<u64>() / (1024 * 1024)
}

pub static PARAKEET_V2: &[StaticFile] = &[
    StaticFile {
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx/resolve/0bbb45a3365852604aef28b538a8f066f4ccaa85/encoder-model.int8.onnx",
        local_name: "encoder-model.int8.onnx",
        sha256: "3e0581fda6ab843888b51e56d7ee78b6d5bc3237ec113af1f732d1d5286aa155",
        size: 652184014,
    },
    StaticFile {
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx/resolve/0bbb45a3365852604aef28b538a8f066f4ccaa85/decoder_joint-model.int8.onnx",
        local_name: "decoder_joint-model.int8.onnx",
        sha256: "a449f49acd68979d418651dd2dcb737cc0f1bf0225e009e29ee326354edbf7d3",
        size: 8998286,
    },
    StaticFile {
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx/resolve/0bbb45a3365852604aef28b538a8f066f4ccaa85/nemo128.onnx",
        local_name: "nemo128.onnx",
        sha256: "a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f",
        size: 139764,
    },
    StaticFile {
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx/resolve/0bbb45a3365852604aef28b538a8f066f4ccaa85/vocab.txt",
        local_name: "vocab.txt",
        sha256: "ec182b70dd42113aff6c5372c75cac58c952443eb22322f57bbd7f53977d497d",
        size: 9384,
    },
    StaticFile {
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx/resolve/0bbb45a3365852604aef28b538a8f066f4ccaa85/config.json",
        local_name: "config.json",
        sha256: "666903c76b9798caf2c210afd4f6cd60b08a8dbf9800ec8d7a3bc0d2148ac466",
        size: 97,
    },
];

pub static PARAKEET_V3: &[StaticFile] = &[
    StaticFile {
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/encoder-model.int8.onnx",
        local_name: "encoder-model.int8.onnx",
        sha256: "6139d2fa7e1b086097b277c7149725edbab89cc7c7ae64b23c741be4055aff09",
        size: 652183999,
    },
    StaticFile {
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/decoder_joint-model.int8.onnx",
        local_name: "decoder_joint-model.int8.onnx",
        sha256: "eea7483ee3d1a30375daedc8ed83e3960c91b098812127a0d99d1c8977667a70",
        size: 18202004,
    },
    StaticFile {
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/nemo128.onnx",
        local_name: "nemo128.onnx",
        sha256: "a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f",
        size: 139764,
    },
    StaticFile {
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/vocab.txt",
        local_name: "vocab.txt",
        sha256: "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
        size: 93939,
    },
    StaticFile {
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/config.json",
        local_name: "config.json",
        sha256: "666903c76b9798caf2c210afd4f6cd60b08a8dbf9800ec8d7a3bc0d2148ac466",
        size: 97,
    },
];

pub static MOONSHINE_BASE: &[StaticFile] = &[
    StaticFile {
        url: "https://huggingface.co/moonshine-ai/moonshine/resolve/48b4e427b587bcf67797a5be706d6ddc4a298149/onnx/merged/base/quantized_4bit/encoder_model.onnx",
        local_name: "encoder_model.onnx",
        sha256: "e6645050b45f5281214fd2788f3d6226cc94149a55c9d503f8b880acb99826a6",
        size: 31027744,
    },
    StaticFile {
        url: "https://huggingface.co/moonshine-ai/moonshine/resolve/48b4e427b587bcf67797a5be706d6ddc4a298149/onnx/merged/base/quantized_4bit/decoder_model_merged.onnx",
        local_name: "decoder_model_merged.onnx",
        sha256: "29bd47951f72e593e0d09aecb81905d20b7892d4574bf23d16e79c0cb2a91e82",
        size: 42427308,
    },
    StaticFile {
        url: "https://huggingface.co/moonshine-ai/moonshine/resolve/48b4e427b587bcf67797a5be706d6ddc4a298149/onnx/merged/base/float/tokenizer.json",
        local_name: "tokenizer.json",
        sha256: "ad3a2ceb0e84e4da57451d86fa337f8116dcff5f5d106434f8aa0b0de89718b9",
        size: 3761751,
    },
];

pub static SENSE_VOICE: &[StaticFile] = &[
    StaticFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/2365baeacb507f821a0c8120fcee3d484dba7a07/model.int8.onnx",
        local_name: "model.int8.onnx",
        sha256: "c71f0ce00bec95b07744e116345e33d8cbbe08cef896382cf907bf4b51a2cd51",
        size: 239233841,
    },
    StaticFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/2365baeacb507f821a0c8120fcee3d484dba7a07/tokens.txt",
        local_name: "tokens.txt",
        sha256: "f449eb28dc567533d7fa59be34e2abca8784f771850c78a47fb731a31429a1dc",
        size: 315894,
    },
];

pub static GIGAAM_V3: &[StaticFile] = &[
    StaticFile {
        url: "https://huggingface.co/istupakov/gigaam-v3-onnx/resolve/322c3b29492673eb7d0b434bfa9dfb8653e34d02/v3_e2e_ctc.int8.onnx",
        local_name: "model.int8.onnx",
        sha256: "2e3fcb7a7b66030336fd10c2fcfb033bd1dc7e1bf238fe5cfd83b1d0cfc9d28e",
        size: 224893347,
    },
    StaticFile {
        url: "https://huggingface.co/istupakov/gigaam-v3-onnx/resolve/322c3b29492673eb7d0b434bfa9dfb8653e34d02/v3_e2e_ctc_vocab.txt",
        local_name: "vocab.txt",
        sha256: "142de7570b3de5b3035ce111a89c228e80e6085273731d944093ddf24fa539cd",
        size: 2007,
    },
];

pub static CANARY_180M: &[StaticFile] = &[
    StaticFile {
        url: "https://huggingface.co/istupakov/canary-180m-flash-onnx/resolve/92c2231a4e2b2524277fea759be967d2e6edfc49/encoder-model.int8.onnx",
        local_name: "encoder-model.int8.onnx",
        sha256: "996d1c89e6cbc891a7c88bf410884c178ffa474f7b13084522ac74a5e144cc81",
        size: 133710896,
    },
    StaticFile {
        url: "https://huggingface.co/istupakov/canary-180m-flash-onnx/resolve/92c2231a4e2b2524277fea759be967d2e6edfc49/decoder-model.int8.onnx",
        local_name: "decoder-model.int8.onnx",
        sha256: "9dd9c447872088c912e916d73751f9621a54085d5bc46788454fe904db51a914",
        size: 79520211,
    },
    StaticFile {
        url: "https://huggingface.co/istupakov/canary-180m-flash-onnx/resolve/92c2231a4e2b2524277fea759be967d2e6edfc49/vocab.txt",
        local_name: "vocab.txt",
        sha256: "2dae6fc7815f9640645e0c765522b278ee0cef49b482d91f6913e334628d3e77",
        size: 53555,
    },
    StaticFile {
        url: "https://huggingface.co/istupakov/canary-180m-flash-onnx/resolve/92c2231a4e2b2524277fea759be967d2e6edfc49/config.json",
        local_name: "config.json",
        sha256: "f90ace8e35326dcd47c7330b230644fa0835083ed1e89e2f59aa08ba10d74f54",
        size: 68,
    },
    StaticFile {
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/nemo128.onnx",
        local_name: "nemo128.onnx",
        sha256: "a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f",
        size: 139764,
    },
];

pub static CANARY_1B_V2: &[StaticFile] = &[
    StaticFile {
        url: "https://huggingface.co/istupakov/canary-1b-v2-onnx/resolve/5ebc1520cef7b6b318b3526ad17adbfe00bc1bfc/encoder-model.int8.onnx",
        local_name: "encoder-model.int8.onnx",
        sha256: "6d96e9945898e5ace48f4efecd459ca1df81859730be27b8af6b197639403ee1",
        size: 859078138,
    },
    StaticFile {
        url: "https://huggingface.co/istupakov/canary-1b-v2-onnx/resolve/5ebc1520cef7b6b318b3526ad17adbfe00bc1bfc/decoder-model.int8.onnx",
        local_name: "decoder-model.int8.onnx",
        sha256: "52d83aa7aad41fbbe4f9dfcd341d784735a6eb4c6eb0d3290fc27a0d8ac39abf",
        size: 170040374,
    },
    StaticFile {
        url: "https://huggingface.co/istupakov/canary-1b-v2-onnx/resolve/5ebc1520cef7b6b318b3526ad17adbfe00bc1bfc/vocab.txt",
        local_name: "vocab.txt",
        sha256: "2c9efe6104fd29522ea27ce0e3aef5d37c690af4e5a4232e643e23ca403ffea3",
        size: 208022,
    },
    StaticFile {
        url: "https://huggingface.co/istupakov/canary-1b-v2-onnx/resolve/5ebc1520cef7b6b318b3526ad17adbfe00bc1bfc/config.json",
        local_name: "config.json",
        sha256: "f90ace8e35326dcd47c7330b230644fa0835083ed1e89e2f59aa08ba10d74f54",
        size: 68,
    },
    StaticFile {
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/nemo128.onnx",
        local_name: "nemo128.onnx",
        sha256: "a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f",
        size: 139764,
    },
];

pub static COHERE: &[StaticFile] = &[
    StaticFile {
        url: "https://huggingface.co/tristanripke/cohere-transcribe-onnx-int8/resolve/9ecc3a5e64b132ab094bada232650e49e4340ad2/cohere-encoder.int8.onnx",
        local_name: "cohere-encoder.int8.onnx",
        sha256: "58386cad715aa0ab30aaa118a479e43115380c114bd180178a0d110434991a54",
        size: 3118156,
    },
    StaticFile {
        url: "https://huggingface.co/tristanripke/cohere-transcribe-onnx-int8/resolve/9ecc3a5e64b132ab094bada232650e49e4340ad2/cohere-encoder.int8.onnx.data",
        local_name: "cohere-encoder.int8.onnx.data",
        sha256: "c115cacd07bef2c5d6bbfa800bb38e6f025ecbfbd220b81b711f0eef8cc28578",
        size: 2732687328,
    },
    StaticFile {
        url: "https://huggingface.co/tristanripke/cohere-transcribe-onnx-int8/resolve/9ecc3a5e64b132ab094bada232650e49e4340ad2/cohere-decoder.int8.onnx",
        local_name: "cohere-decoder.int8.onnx",
        sha256: "8372ca6c8ff4db8b916ca3592f5c757a715e691b9edec751ba19b29fc854baf9",
        size: 153250705,
    },
    StaticFile {
        url: "https://huggingface.co/csukuangfj2/sherpa-onnx-cohere-transcribe-14-lang-int8-2026-04-01/resolve/156a470cf08eefe706a0004f3c52d9ee567ca7a0/tokens.txt",
        local_name: "tokens.txt",
        sha256: "013ede043ae2480e3a9205cc34550d9686100cc682bacc90f702facdfbb93035",
        size: 207437,
    },
];
