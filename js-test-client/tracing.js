const opentelemetry = require("@opentelemetry/api");
const { Resource } = require("@opentelemetry/resources");
const { SemanticResourceAttributes } = require("@opentelemetry/semantic-conventions");
const { OTLPTraceExporter } = require("@opentelemetry/exporter-trace-otlp-grpc");
const { BasicTracerProvider, SimpleSpanProcessor } = require('@opentelemetry/sdk-trace-base');
const { AsyncHooksContextManager } = require("@opentelemetry/context-async-hooks");
const uuid = require('uuid');

const contextManager = new AsyncHooksContextManager();
contextManager.enable();
opentelemetry.context.setGlobalContextManager(contextManager);

const resource = Resource.default().merge(
    new Resource({
        [SemanticResourceAttributes.SERVICE_NAME]: "js-otlp-client",
        [SemanticResourceAttributes.SERVICE_NAMESPACE]: "modality-otlp-test",
        [SemanticResourceAttributes.SERVICE_VERSION]: "0.1.0",
        [SemanticResourceAttributes.SERVICE_INSTANCE_ID]: uuid.v4(), 
    })
);

const provider = new BasicTracerProvider(({ resource }));
const exporter = new OTLPTraceExporter({ url: 'http://localhost:4317' });
provider.addSpanProcessor(new SimpleSpanProcessor(exporter));

provider.register();
['SIGINT', 'SIGTERM'].forEach(signal => {
    process.on(signal, () => provider.shutdown().catch(console.error));
});
