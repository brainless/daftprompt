use model2vec_rs::model::StaticModel;

pub const DEFAULT_MODEL: &str = "minishlab/potion-code-16M";

pub struct Embedder {
    model: StaticModel,
    pub dimension: usize,
}

impl Embedder {
    pub fn new(model_name: &str) -> anyhow::Result<Self> {
        let model = StaticModel::from_pretrained(model_name, None, None, None)?;
        let probe = model.encode_single("probe");
        let dimension = probe.len();
        Ok(Self { model, dimension })
    }

    pub fn encode_batch(&self, texts: &[String]) -> Vec<Vec<f32>> {
        self.model.encode(texts)
    }

    pub fn encode_single(&self, text: &str) -> Vec<f32> {
        self.model.encode_single(text)
    }
}
