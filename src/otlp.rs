use std::net::SocketAddr;

use opentelemetry::proto::collector::trace::v1::{
    trace_service_server::{TraceService, TraceServiceServer},
    ExportTraceServiceRequest, ExportTraceServiceResponse,
};
use tokio::sync::mpsc::Sender;
use tracing::error;

use self::opentelemetry::proto::trace::v1::ResourceSpans;

pub async fn run_receiver(
    addr: SocketAddr,
    message_sender: Sender<Vec<ResourceSpans>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tonic::transport::Server::builder()
        .add_service(TraceServiceServer::new(Receiver { message_sender }))
        .serve(addr)
        .await?;

    Ok(())
}

struct Receiver {
    message_sender: Sender<Vec<ResourceSpans>>,
}

#[allow(unused)]
#[tonic::async_trait]
impl TraceService for Receiver {
    async fn export(
        &self,
        request: tonic::Request<ExportTraceServiceRequest>,
    ) -> Result<tonic::Response<ExportTraceServiceResponse>, tonic::Status> {
        if let Err(e) = self
            .message_sender
            .send(request.into_inner().resource_spans).await
        {
            error!(
                err = &e as &dyn std::error::Error,
                "Error sending message on internal channel"
            );
        }

        Ok(tonic::Response::new(ExportTraceServiceResponse {
            partial_success: None,
        }))
    }
}

pub mod opentelemetry {
    pub mod proto {
        pub mod collector {
            pub mod trace {
                pub mod v1 {
                    tonic::include_proto!("opentelemetry.proto.collector.trace.v1");
                }
            }
        }
        pub mod common {
            pub mod v1 {
                tonic::include_proto!("opentelemetry.proto.common.v1");
            }
        }
        pub mod resource {
            pub mod v1 {
                tonic::include_proto!("opentelemetry.proto.resource.v1");
            }
        }
        pub mod trace {
            pub mod v1 {
                tonic::include_proto!("opentelemetry.proto.trace.v1");
            }
        }
    }
}
