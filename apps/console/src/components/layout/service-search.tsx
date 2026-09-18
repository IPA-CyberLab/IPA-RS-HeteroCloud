import Autosuggest from "@cloudscape-design/components/autosuggest";
import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { consoleServices } from "./service-catalog";

const options = consoleServices.map((service) => ({
  value: service.id,
  label: service.name,
  description: service.shortName,
  labelTag: service.group,
  filteringTags: service.keywords,
}));

export function ServiceSearch({
  ariaLabel = "サービスを検索",
  placeholder = "サービスを検索",
}: {
  ariaLabel?: string;
  placeholder?: string;
}) {
  const [value, setValue] = useState("");
  const navigate = useNavigate();

  return (
    <Autosuggest
      ariaLabel={ariaLabel}
      value={value}
      options={options}
      placeholder={placeholder}
      filteringType="auto"
      hideEnteredTextOption
      empty="一致するサービスがありません"
      selectedAriaLabel="選択済み"
      onChange={({ detail }) => setValue(detail.value)}
      onSelect={({ detail }) => {
        const service = consoleServices.find((item) => item.id === detail.value);
        if (!service) return;
        setValue("");
        navigate(service.href);
      }}
    />
  );
}
