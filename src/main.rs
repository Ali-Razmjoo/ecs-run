extern crate clap;
extern crate rusoto_core;
extern crate rusoto_ecs;
extern crate rusoto_logs;

use clap::{App, Arg};
use rusoto_core::Region;
use rusoto_ecs::{Ecs, EcsClient};
use rusoto_logs::{CloudWatchLogs, CloudWatchLogsClient};
use std::str::FromStr;
use std::{thread, time};

fn main() {
    let matches = App::new("ecs-run")
        .version("0.1.0")
        .author("Erik Dalén <erik.gustav.dalen@gmail.com>")
        .arg(
            Arg::with_name("CLUSTER")
                .help("Name of cluster to run in")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::with_name("SERVICE")
                .help("Service to base task on")
                .required(true)
                .index(2),
        )
        .get_matches();

    let cluster = matches.value_of("CLUSTER").unwrap();
    let service = matches.value_of("SERVICE").unwrap();

    let ecs_client = EcsClient::simple(Region::default());
    match fetch_service(&ecs_client, &cluster, &service) {
        Ok(service) => {
            let task_definition = fetch_task_definition(&ecs_client, &service)
                .unwrap()
                .task_definition
                .unwrap();
            let container = get_container(&task_definition);

            let log_options = container.log_configuration.unwrap().options.unwrap();
            let log_group = log_options
                .get("awslogs-group")
                .expect("No log group configured");
            let log_region = log_options
                .get("awslogs-region")
                .expect("No log region configured");
            let log_prefix = log_options
                .get("awslogs-stream-prefix")
                .expect("No log stream prefix configured");

            let task = run_task(
                &ecs_client,
                &cluster.to_string(),
                &task_definition,
                &service,
            );
            let task_id = &task.clone()
                .task_arn
                .unwrap()
                .rsplitn(2, "/")
                .next()
                .unwrap()
                .to_string();

            println!("Started task {}", &task_id);
            loop {
                let task_status = fetch_task(&ecs_client, &cluster.to_string(), &task);
                if task_status.stopped_at != None {
                    break;
                }
                thread::sleep(time::Duration::from_millis(500));
            }
            println!("Task finished, fetching logs");

            let log_stream_name =
                format!("{}/{}/{}", &log_prefix, &container.name.unwrap(), &task_id);
            let logs_client = CloudWatchLogsClient::simple(Region::from_str(&log_region).unwrap());
            let logs = fetch_logs(&logs_client, &log_group, &log_stream_name);

            println!("logs: {:?}", &logs);
            println!("task: {:?}", &task);
        }
        Err(error) => {
            println!("Error: {:?}", error);
        }
    }
}

// TODO: loop if there are more logs
fn fetch_logs(
    client: &rusoto_logs::CloudWatchLogsClient,
    log_group_name: &String,
    log_stream_name: &String,
) -> rusoto_logs::GetLogEventsResponse {
    println!("log_group: {}", &log_group_name);
    println!("log_stream: {}", &log_stream_name);
    let result = client
        .get_log_events(&rusoto_logs::GetLogEventsRequest {
            log_group_name: log_group_name.clone(),
            log_stream_name: log_stream_name.clone(),
            ..Default::default()
        })
        .sync();
    result.unwrap()
}

fn fetch_task(client: &EcsClient, cluster: &String, task: &rusoto_ecs::Task) -> rusoto_ecs::Task {
    let result = client
        .describe_tasks(&rusoto_ecs::DescribeTasksRequest {
            cluster: Some(cluster.clone()),
            tasks: vec![task.clone().task_arn.unwrap()],
        })
        .sync();
    result.unwrap().tasks.unwrap()[0].clone()
}

// TODO: allowoverriding which container
fn get_container(task_definition: &rusoto_ecs::TaskDefinition) -> rusoto_ecs::ContainerDefinition {
    task_definition
        .clone()
        .container_definitions
        .unwrap_or_default()[0]
        .clone()
}

fn run_task(
    client: &EcsClient,
    cluster: &String,
    task_definition: &rusoto_ecs::TaskDefinition,
    service: &rusoto_ecs::Service,
) -> rusoto_ecs::Task {
    let service = service.clone();
    let container = get_container(&task_definition);
    let result = client
        .run_task(&rusoto_ecs::RunTaskRequest {
            cluster: Some(cluster.to_string()),
            count: Some(1),
            launch_type: service.launch_type,
            network_configuration: service.network_configuration,
            placement_constraints: service.placement_constraints,
            placement_strategy: service.placement_strategy,
            platform_version: service.platform_version,
            task_definition: service
                .task_definition
                .expect("No task definition in service"),
            overrides: Some(rusoto_ecs::TaskOverride {
                container_overrides: Some(vec![rusoto_ecs::ContainerOverride {
                    name: container.name.clone(),
                    command: Some(vec!["rake".to_string(), "-t".to_string()]),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            started_by: Some("ecs-run".to_string()),
            ..Default::default()
        })
        .sync();
    result.unwrap().tasks.unwrap()[0].clone()
}

fn fetch_task_definition(
    client: &EcsClient,
    service: &rusoto_ecs::Service,
) -> Result<rusoto_ecs::DescribeTaskDefinitionResponse, rusoto_ecs::DescribeTaskDefinitionError> {
    client
        .describe_task_definition(&rusoto_ecs::DescribeTaskDefinitionRequest {
            task_definition: service.clone().task_definition.unwrap(),
        })
        .sync()
}

fn fetch_service(
    client: &EcsClient,
    cluster: &str,
    service: &str,
) -> Result<rusoto_ecs::Service, String> {
    match client
        .describe_services(&rusoto_ecs::DescribeServicesRequest {
            cluster: Some(cluster.to_string()),
            services: vec![service.to_string()],
        })
        .sync()
    {
        Ok(response) => match response.services {
            Some(services) => {
                return Ok(services[0].clone());
            }
            None => Err(format!("Could not find service {}", &service)),
        },
        Err(error) => Err(format!("Error: {:?}", &error)),
    }
}
