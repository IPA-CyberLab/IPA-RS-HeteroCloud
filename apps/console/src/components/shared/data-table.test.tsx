import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ColumnDef } from "@tanstack/react-table";
import { describe, expect, it, vi } from "vitest";
import { DataTable } from "@/components/shared/data-table";

interface RowData {
  id: string;
  name: string;
}

const columns: ColumnDef<RowData, unknown>[] = [
  {
    accessorKey: "name",
    header: "名前",
  },
];

describe("DataTable", () => {
  it("検索結果だけを表示する", async () => {
    const user = userEvent.setup();
    const data = [
      { id: "1", name: "Production" },
      { id: "2", name: "Staging" },
      { id: "3", name: "Development" },
    ];

    render(<DataTable columns={columns} data={data} getRowId={(row) => row.id} />);
    await user.type(screen.getByLabelText("テーブルを検索"), "Staging");

    expect(screen.getByText("Staging")).toBeInTheDocument();
    expect(screen.queryByText("Production")).not.toBeInTheDocument();
    expect(screen.getByText("1 件")).toBeInTheDocument();
  });

  it("ソートとページングを操作できる", async () => {
    const user = userEvent.setup();
    const data = Array.from({ length: 12 }, (_, index) => ({
      id: String(index),
      name: `Resource ${String(12 - index).padStart(2, "0")}`,
    }));

    render(<DataTable columns={columns} data={data} getRowId={(row) => row.id} />);

    await user.click(screen.getByRole("button", { name: /名前/ }));
    let bodyRows = within(screen.getByRole("table")).getAllByRole("row").slice(1);
    expect(bodyRows[0]).toHaveTextContent("Resource 01");

    await user.click(screen.getByRole("button", { name: "次のページ" }));
    bodyRows = within(screen.getByRole("table")).getAllByRole("row").slice(1);
    expect(bodyRows).toHaveLength(2);
    expect(bodyRows[0]).toHaveTextContent("Resource 11");
  });

  it("行全体をマウスとキーボードで開き、行内操作は横取りしない", async () => {
    const user = userEvent.setup();
    const onRowClick = vi.fn();
    const interactiveColumns: ColumnDef<RowData, unknown>[] = [
      ...columns,
      {
        id: "action",
        header: "",
        cell: () => <button type="button">操作</button>,
      },
    ];
    const data = [{ id: "1", name: "Production" }];

    render(
      <DataTable
        columns={interactiveColumns}
        data={data}
        getRowId={(row) => row.id}
        onRowClick={onRowClick}
        getRowAriaLabel={(row) => `${row.name}の詳細を開く`}
      />,
    );

    const row = screen.getByRole("row", { name: /Production/ });
    await user.click(within(row).getByText("Production"));
    expect(onRowClick).toHaveBeenCalledWith(data[0]);

    onRowClick.mockClear();
    await user.click(within(row).getByRole("button", { name: "操作" }));
    expect(onRowClick).not.toHaveBeenCalled();

    within(row).getByRole("link", { name: "Productionの詳細を開く" }).focus();
    await user.keyboard("{Enter}");
    expect(onRowClick).toHaveBeenCalledWith(data[0]);
  });
});
