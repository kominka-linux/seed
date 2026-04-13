use crate::common::error::AppletError;

pub type AppletResult = Result<(), Vec<AppletError>>;
pub type AppletCodeResult = Result<i32, Vec<AppletError>>;

pub fn fail(errors: Vec<AppletError>, code: i32) -> i32 {
    for error in errors {
        error.print();
    }
    code
}

pub fn finish(result: AppletResult) -> i32 {
    finish_or(result, 1)
}

pub fn finish_or(result: AppletResult, err_code: i32) -> i32 {
    match result {
        Ok(()) => 0,
        Err(errors) => fail(errors, err_code),
    }
}

pub fn finish_code(result: AppletCodeResult) -> i32 {
    finish_code_or(result, 1)
}

pub fn finish_code_or(result: AppletCodeResult, err_code: i32) -> i32 {
    match result {
        Ok(code) => code,
        Err(errors) => fail(errors, err_code),
    }
}
