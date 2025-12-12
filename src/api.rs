use crate::message::EvalResult;


pub trait JuKernel {
    fn kernel_info(&self) -> JuKernelInfo;
    fn eval_code(&mut self, code: String) -> impl std::future::Future<Output = EvalResult>;
}

pub struct JuKernelInfo {
    pub name: String,
    pub version: String,
    pub mimetype: String,
    pub file_extension: String,
    pub banner: String,
    pub help_links: Vec<JuHelpLink>,
}

pub struct JuHelpLink {
    pub text: String,
    pub url: String,
}
