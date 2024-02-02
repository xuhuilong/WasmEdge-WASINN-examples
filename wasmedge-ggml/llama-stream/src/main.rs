use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::io::{self, Write};
use wasi_nn::{self, GraphExecutionContext};

fn read_input() -> String {
    loop {
        let mut answer = String::new();
        io::stdin()
            .read_line(&mut answer)
            .expect("Failed to read line");
        if !answer.is_empty() && answer != "\n" && answer != "\r\n" {
            return answer.trim().to_string();
        }
    }
}

fn set_data_to_context(
    context: &mut GraphExecutionContext,
    data: Vec<u8>,
) -> Result<(), wasi_nn::Error> {
    context.set_input(0, wasi_nn::TensorType::U8, &[1], &data)
}

#[allow(dead_code)]
fn set_metadata_to_context(
    context: &mut GraphExecutionContext,
    data: Vec<u8>,
) -> Result<(), wasi_nn::Error> {
    context.set_input(1, wasi_nn::TensorType::U8, &[1], &data)
}

fn get_data_from_context(context: &GraphExecutionContext, index: usize, is_single: bool) -> String {
    // Preserve for 4096 tokens with average token length 6
    const MAX_OUTPUT_BUFFER_SIZE: usize = 4096 * 6;
    let mut output_buffer = vec![0u8; MAX_OUTPUT_BUFFER_SIZE];
    let mut output_size = if is_single {
        context
            .get_output_single(index, &mut output_buffer)
            .expect("Failed to get single output")
    } else {
        context
            .get_output(index, &mut output_buffer)
            .expect("Failed to get output")
    };
    output_size = std::cmp::min(MAX_OUTPUT_BUFFER_SIZE, output_size);

    return String::from_utf8_lossy(&output_buffer[..output_size]).to_string();
}

#[allow(dead_code)]
fn get_output_from_context(context: &GraphExecutionContext) -> String {
    return get_data_from_context(context, 0, false);
}

fn get_single_output_from_context(context: &GraphExecutionContext) -> String {
    return get_data_from_context(context, 0, true);
}

#[allow(dead_code)]
fn get_metadata_from_context(context: &GraphExecutionContext) -> Value {
    return serde_json::from_str(&get_data_from_context(context, 1, false))
        .expect("Failed to get metadata");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let model_name: &str = &args[1];

    // Set options for the graph. Check our README for more details:
    // https://github.com/second-state/WasmEdge-WASINN-examples/tree/master/wasmedge-ggml#parameters
    let mut options = HashMap::new();
    options.insert("enable-log", Value::from(false));
    options.insert("n-gpu-layers", Value::from(0));
    options.insert("ctx-size", Value::from(512));

    // Create graph and initialize context.
    let graph =
        wasi_nn::GraphBuilder::new(wasi_nn::GraphEncoding::Ggml, wasi_nn::ExecutionTarget::AUTO)
            .config(serde_json::to_string(&options).expect("Failed to serialize options"))
            .build_from_cache(model_name)
            .expect("Failed to build graph");
    let mut context = graph
        .init_execution_context()
        .expect("Failed to init context");

    // We also support setting the options via input tensor with index 1.
    // Uncomment the line below to run the example, Check our README for more details.
    // set_metadata_to_context(
    //     &mut context,
    //     serde_json::to_string(&options)
    //         .expect("Failed to serialize options")
    //         .as_bytes()
    //         .to_vec(),
    // )
    // .expect("Failed to set metadata");

    let mut saved_prompt = String::new();
    let system_prompt = String::from("You are a helpful, respectful and honest assistant. Always answer as short as possible, while being safe." );

    loop {
        println!("Question:");
        let input = read_input();
        if saved_prompt == "" {
            saved_prompt = format!(
                "[INST] <<SYS>> {} <</SYS>> {} [/INST]",
                system_prompt, input
            );
        } else {
            saved_prompt = format!("{} [INST] {} [/INST]", saved_prompt, input);
        }

        // Set prompt to the input tensor.
        set_data_to_context(&mut context, saved_prompt.as_bytes().to_vec())
            .expect("Failed to set input");

        // Get the number of input tokens and llama.cpp versions.
        // let input_metadata = get_metadata_from_context(&context);
        // println!("[INFO] llama_commit: {}", input_metadata["llama_commit"]);
        // println!(
        //     "[INFO] llama_build_number: {}",
        //     input_metadata["llama_build_number"]
        // );
        // println!(
        //     "[INFO] Number of input tokens: {}",
        //     input_metadata["input_tokens"]
        // );

        // Execute the inference (streaming mode).
        let mut output = String::new();
        let mut reset_prompt = false;
        println!("Answer:");
        loop {
            match context.compute_single() {
                Ok(_) => (),
                Err(wasi_nn::Error::BackendError(wasi_nn::BackendError::EndOfSequence)) => {
                    break;
                }
                Err(wasi_nn::Error::BackendError(wasi_nn::BackendError::ContextFull)) => {
                    println!("\n[INFO] Context full, we'll reset the context and continue.");
                    reset_prompt = true;
                    break;
                }
                Err(wasi_nn::Error::BackendError(wasi_nn::BackendError::PromptTooLong)) => {
                    println!("\n[INFO] Prompt too long, we'll reset the context and continue.");
                    reset_prompt = true;
                    break;
                }
                Err(err) => {
                    println!("\n[ERROR] {}", err);
                    break;
                }
            }
            // Retrieve the single output token and print it.
            let token = get_single_output_from_context(&context);
            print!("{}", token);
            io::stdout().flush().unwrap();
            output += &token;
        }
        println!("");

        // Update the saved prompt.
        if reset_prompt {
            saved_prompt.clear();
        } else {
            output = output.trim().to_string();
            saved_prompt = format!("{} {}", saved_prompt, output);
        }

        // Retrieve the output metadata.
        // let metadata = get_metadata_from_context(&context);
        // println!(
        //     "[INFO] Number of input tokens: {}",
        //     metadata["input_tokens"]
        // );
        // println!(
        //     "[INFO] Number of output tokens: {}",
        //     metadata["output_tokens"]
        // );

        // Delete the context in compute_single mode.
        context.fini_single().unwrap();
    }
}