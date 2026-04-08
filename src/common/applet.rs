use crate::common::error::AppletError;

pub type AppletResult = Result<(), Vec<AppletError>>;

pub fn finish(result: AppletResult) -> i32 {
    match result {
        Ok(()) => 0,
        Err(errors) => {
            for error in errors {
                error.print();
            }
            1
        }
    }
}
